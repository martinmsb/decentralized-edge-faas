use actix_web::http::StatusCode;
use actix_web::{web, HttpResponse, Responder};
use actix_multipart::Multipart;
use reqwest::Method;
use futures::FutureExt;
use libp2p::PeerId;
use tokio::sync::Mutex;
use tokio::task::spawn;

use std::sync::Arc;
use std::collections::HashSet;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::json;
use uuid::Uuid;

use crate::data_structures::RequestsInProgress;
use crate::http_server::server::AppState;
use crate::network::NetworkClient;
use crate::openfaas::OpenFaasClient;

#[derive(Deserialize)]
pub struct AnycallBody {
    http_method: String,
    body: Option<Value>,
    path_and_query: Option<String>
}

#[derive(Deserialize)]
pub struct ManycallBody {
    // Array of objects
    items: Value
}

pub struct OpenFaaSResponse {
    status: u16,
    body: Vec<u8>
}

pub async fn execute_function(data: web::Data<AppState>, path: web::Path<String>, req_body: web::Json<AnycallBody>) -> impl Responder {
    let mut name = path.into_inner();
    if name.is_empty() {
        return Err(actix_web::error::ErrorBadRequest("Function name is empty"));
    }
    
    let network_client = &data.nc;
    let openfaas_client = &data.ofc;
    let requests_in_progress = &data.rp;
    let peer_id = &data.peer_id;

    let method = &req_body.http_method;
    
    match Method::from_bytes(method.as_bytes()) {
        Ok(_) => (),
        Err(_) => return Err(actix_web::error::ErrorBadRequest("Invalid HTTP method"))
    };

    let body_field = &req_body.body;
    let body = match body_field {
        Some(b) => Some(b.to_string().into_bytes()),
        None => None
    };

    let path_and_query_field = &req_body.path_and_query;

    let function_response_result;
    // Locate all nodes providing the function.
    let providers = network_client.get_providers(name.clone()).await;
    println!("providers: {:?}", providers);
    if providers.is_empty() {
        return Err(actix_web::error::ErrorInternalServerError("Could not find provider"));
    }
    
    if path_and_query_field.is_some() {
        name = name + path_and_query_field.as_ref().unwrap();
    }

    // Request the content of the file from each node.
    println!("providers: {:?}", providers);
    // Check if providers length is 1 and then check if it is the same as the peer_id
    // If it is, then return the file content
    match function_request(providers, peer_id, &name, &method, &body, requests_in_progress, openfaas_client, network_client).await
    {
        Ok(resp) => function_response_result = resp,
        Err(e) => return Err(e)
    }
    
    let s = match String::from_utf8(function_response_result.body) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Invalid UTF-8 sequence: {:?}", e);
            return Err(actix_web::error::ErrorInternalServerError("Invalid UTF-8 sequence"))
        }
    };
    
    Ok(HttpResponse::build(StatusCode::from_u16(function_response_result.status).unwrap()).body(s))

}

pub async fn execute_function_manycall(data: web::Data<AppState>, path: web::Path<String>, req_body: web::Json<ManycallBody>) -> impl Responder {
    let name = path.into_inner();
    if name.is_empty() {
        return Err(actix_web::error::ErrorBadRequest("Function name is empty"));
    }
    
    let network_client = &data.nc;
    let openfaas_client = &data.ofc;
    let peer_id = &data.peer_id;
    let requests_in_progress = &data.rp;

    // TODO uncomment
    let providers = network_client.get_providers(name.clone()).await;
    println!("providers: {:?}", providers);

    if providers.is_empty() {
        return Err(actix_web::error::ErrorInternalServerError("Could not find provider"));
    }
    
    let items = match req_body.items {
        Value::Array(ref array) => {
            // Handle the case where the item is an array
            array.to_owned()
        }
        _ => return Ok(HttpResponse::BadRequest().body("Invalid request body. Field items must be an array")),
    };

    
    let items_result: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(vec![serde_json::Value::String("".to_string()); items.len()]));

    let mut providers_iter = providers.iter();
    let mut actual_item = 0;
    let mut handles = vec![];
    let items_clone = items.clone();
    
    for item in items_clone {
        // TODO remove clone here as used in other scope
        let network_client_clone = network_client.clone();
        let openfaas_client_clone = Arc::clone(openfaas_client);
        let peer_id_clone = peer_id.clone();
        let requests_in_progress_clone = Arc::clone(requests_in_progress);
        let providers_clone: HashSet<PeerId> = providers.clone();
        let name_clone = name.clone();
        let method = "POST".to_string();
        let shared_result         = Arc::clone(&items_result);

        let provider_i = match providers_iter.next() {
            Some(p) => Some(p.to_owned()),
            None => None
        };
        // Spawn a new asynchronous task
        let handle_call = spawn(async move {
            let provider: PeerId;
            {
                // In the closure, lock the Mutex to get access to the Requests in Progress to unlock it after the closure
                let mut rp_instance = requests_in_progress_clone.lock().await;
                match provider_i {
                    Some(p) => {
                        rp_instance.push_req(&p, true);
                        provider = p;
                    }
                    None => {
                        // Get one from struct
                        provider = rp_instance.get_peer(&providers_clone).unwrap();
                        rp_instance.push_req(&provider, false);
                    }
                }
            }
            let body = item.to_string().into_bytes();
            let function_response_status;
            let function_response_result;
            if provider == peer_id_clone {
                let resp;
                
                resp = openfaas_client_clone.request_function(&name_clone, &method, Some(body)).await;
                
                match resp {
                    Ok(resp) => {
                        function_response_status = resp.status().as_u16();
                        let resp_body = resp.bytes().await.unwrap().to_vec();
                        function_response_result = resp_body;
                    }
                    Err(err) => {
                        eprintln!("Failed to get response from function {}: {:?}", name_clone, err);
                        function_response_status = 500;
                        function_response_result = "Failed to get response from function".to_string().into_bytes();
                    }
                }
            }
            else {
                let body = body.clone();
                let function_response;

                function_response = network_client_clone.request_function(provider, name_clone, method, Some(body)).await;
                
                match function_response {
                    Ok(function_response) => {
                        function_response_status = function_response.0;
                        function_response_result = function_response.1
                    },
                    Err(e) => {
                        eprintln!("Response from provider failed: {:?}", e);
                        function_response_status = 500;
                        function_response_result = "Response from provider failed".to_string().into_bytes()
                    }
                };
            }
            {
                let mut rp_instance = requests_in_progress_clone.lock().await;
                rp_instance.pop_req(&provider, true);
            }

            let parsed_body = detect_and_parse_body(function_response_result);
            // Lock the Mutex to get access to the vector
            let mut results = shared_result.lock().await;

            // Write to the shared vector
            if function_response_status != 200 {
                results[actual_item] = json!({
                    "status": function_response_status,
                    "body": serialize_body(parsed_body)
                });
            } else {
                results[actual_item] = serialize_body(parsed_body);
            }
        });
        // Collect task handles to await them later
        handles.push(handle_call);
        actual_item += 1;
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Decrease in progress requests from providers in this manycall
    {
        let mut rp_instance = requests_in_progress.lock().await;
        rp_instance.remove_manycall(&providers);
    }

    let results = items_result.lock().await;

    let response = results.to_owned();

    let body = json!({
        "results": response
    });

    Ok(HttpResponse::Ok().json(body))
}

pub async fn deploy_function(data: web::Data<AppState>, payload: Multipart) -> impl Responder {
    let my_uuid = Uuid::new_v4();
    // add fn- to uuid to create funciton name
    let function_name = "fn-".to_owned() + &my_uuid.to_string();
    let openfaas_client = &data.ofc;
    let network_client = &data.nc;
    
    // Deploy the function to openfaas
    let openfaas_deploy_result = openfaas_client.deploy_function(&function_name, payload).await;
    match openfaas_deploy_result {
        Ok(_) => (),
        Err(e) => {
            eprintln!("Failed to deploy function: {:?}", e);
            return Err(actix_web::error::ErrorInternalServerError("Failed to deploy function"));
        }
    }

    // Start providing the function name to the network.    
    network_client.start_providing(function_name.clone()).await;
    
    Ok(HttpResponse::Ok().body(function_name))
}

pub async fn deploy_known_function(data: web::Data<AppState>, payload: Multipart, path: web::Path<String>) -> impl Responder {
    let function_name = path.into_inner();
    if function_name.is_empty() {
        return Err(actix_web::error::ErrorBadRequest("Function name is empty"));
    }
    let openfaas_client = &data.ofc;
    let network_client = &data.nc;
    
    // Deploy the function to openfaas
    let openfaas_deploy_result = deploy_openfaas(openfaas_client, &function_name, payload).await;
    match openfaas_deploy_result {
        Ok(_) => (),
        Err(e) => {
            eprintln!("Failed to deploy function: {:?}", e);
            return Err(actix_web::error::ErrorInternalServerError("Failed to deploy function"));
        }
    }    

    // Start providing the function name to the network.    
    anounce_provider(network_client, &function_name).await;
    
    Ok(HttpResponse::Ok().body(function_name))
}

async fn deploy_openfaas(openfaas_client: &Arc<OpenFaasClient>, function_name: &String, payload: Multipart) -> Result<(), actix_web::Error> {
    let openfaas_deploy_result = openfaas_client.deploy_function(&function_name, payload).await;
    match openfaas_deploy_result {
        Ok(_) => Ok(()),
        Err(e) => {
            eprintln!("Failed to deploy function: {:?}", e);
            return Err(actix_web::error::ErrorInternalServerError("Failed to deploy function"));
        }
    }
}

async fn anounce_provider(network_client: &Arc<NetworkClient>, function_name: &String) {
    network_client.start_providing(function_name.clone()).await;
}

async fn function_request(providers: HashSet<PeerId>, peer_id: &PeerId, name: &String, method: &String, body: &Option<Vec<u8>>, requests_in_progress: &Arc<Mutex<RequestsInProgress>>, openfaas_client: &Arc<OpenFaasClient>, network_client: &Arc<NetworkClient>) -> Result<OpenFaaSResponse, actix_web::Error> {
    let function_response_status;
    let function_response_body;
    
    if providers.contains(peer_id) {
        let resp = openfaas_client.request_function(&name, &method, body.clone()).await;
        match resp {
            Ok(resp) => {
                function_response_status = resp.status().as_u16();
                let resp_body = resp.bytes().await.unwrap().to_vec();
                function_response_body = resp_body;
            }
            Err(err) => {
                eprintln!("Failed to get response from function {}: {:?}", name, err);
                return Err(actix_web::error::ErrorInternalServerError("Failed to get response from function"));
            }
        }
    }
    else {
        let requests = providers.into_iter().map(|p| {
            let network_client = network_client.clone();
            let name = name.clone();
            let method = method.clone();
            let body = body.clone();
            let requests_in_progress_clone = Arc::clone(requests_in_progress);
            async move { 
                {
                    let mut rp_instance = requests_in_progress_clone.lock().await;
                    rp_instance.push_req(&p, false);
                }
                let response = network_client.request_function(p, name, method, body).await;
                {
                    let mut rp_instance = requests_in_progress_clone.lock().await;
                    rp_instance.pop_req(&p, false);
                }
                response
             }.boxed()
        });

        // Await the requests, ignore the remaining once a single one succeeds.
        let function_response = futures::future::select_ok(requests)
            .await;
        
        match function_response {
            Ok(function_response) => {
                function_response_body = function_response.0.1;
                function_response_status = function_response.0.0;
            },
            Err(e) => {
                eprintln!("None of the providers responded: {:?}", e);
                return Err(actix_web::error::ErrorInternalServerError("None of the providers responded"));
            }
        };
    }
    let openfaas_response = OpenFaaSResponse {
        status: function_response_status,
        body: function_response_body
    };
    Ok(openfaas_response)
}

#[derive(Clone, Serialize)]
enum BodyData {
    Integer(i32),
    Float(f64),
    String(String),
    Boolean(bool),
    Json(Value),
}

fn detect_and_parse_body(body: Vec<u8>) -> BodyData {
    // Convert Vec<u8> to String
    let string_body = match String::from_utf8(body.clone()) {
        Ok(s) => s,
        Err(_) => return BodyData::String("Invalid UTF-8 data".to_string()),
    };

    // Try to parse as JSON
    if let Ok(json_value) = serde_json::from_str::<Value>(&string_body) {
        return BodyData::Json(json_value);
    }

    // Try to parse as integer
    if let Ok(int_value) = string_body.parse::<i32>() {
        return BodyData::Integer(int_value);
    }

    // Try to parse as float
    if let Ok(float_value) = string_body.parse::<f64>() {
        return BodyData::Float(float_value);
    }

    // Try to parse as boolean
    if let Ok(bool_value) = string_body.parse::<bool>() {
        return BodyData::Boolean(bool_value);
    }

    // Default to string if not an integer, float, boolean, or JSON
    BodyData::String(string_body)
}

fn serialize_body(body: BodyData) -> serde_json::Value {
    let serialized_data: serde_json::Value = 
        match body {
            BodyData::Integer(val) => json!(val),
            BodyData::Float(val) => json!(val),
            BodyData::String(val) => json!(val),
            BodyData::Boolean(val) => json!(val),
            BodyData::Json(val) => val,
        };

    json!(serialized_data)
    
}
