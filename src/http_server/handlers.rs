use actix_web::{web, HttpResponse, Responder};
use actix_multipart::Multipart;
use futures::FutureExt;
use libp2p::PeerId;
use tokio::sync::Mutex;
use tokio::task::spawn;

use std::sync::Arc;
use std::collections::HashSet;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::data_structures::RequestsInProgress;
use crate::http_server::server::AppState;
use crate::network::NetworkClient;
use crate::openfaas::OpenFaasClient;

#[derive(Deserialize)]
pub struct AnycallBody {
    http_method: String,
    body: Option<Vec<u8>>
}

#[derive(Deserialize)]
pub struct MulticallBody {
    // Array of objects
    items: Value
}

pub async fn execute_function(data: web::Data<AppState>, path: web::Path<String>, req_body: web::Json<AnycallBody>) -> impl Responder {
    let name = path.into_inner();
    if name.is_empty() {
        return Err(actix_web::error::ErrorBadRequest("Function name is empty"));
    }
    
    let network_client = &data.nc;
    let openfaas_client = &data.ofc;
    let requests_in_progress = &data.rp;
    let peer_id = &data.peer_id;

    let action = &req_body.http_method;
    if action != "GET" && action != "POST" {
        return Err(actix_web::error::ErrorBadRequest("Invalid HTTP method"));
    }

    let body = &req_body.body;

    let function_response_result;
    // Locate all nodes providing the function.
    let providers = network_client.lock().await.get_providers(name.clone()).await;
    println!("providers: {:?}", providers);
    if providers.is_empty() {
        return Err(actix_web::error::ErrorInternalServerError("Could not find provider"));
    }

    // Request the content of the file from each node.
    println!("providers: {:?}", providers);
    // Check if providers length is 1 and then check if it is the same as the peer_id
    // If it is, then return the file content
    match function_request(providers, peer_id, &name, body, requests_in_progress, openfaas_client, network_client).await
    {
        Ok(resp) => function_response_result = resp,
        Err(e) => return Err(e)
    }
    
    let s = match String::from_utf8(function_response_result) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Invalid UTF-8 sequence: {:?}", e);
            return Err(actix_web::error::ErrorInternalServerError("Invalid UTF-8 sequence"))
        }
    };

    Ok(HttpResponse::Ok().body(s))

}

pub async fn execute_function_multicall(data: web::Data<AppState>, path: web::Path<String>, req_body: web::Json<MulticallBody>) -> impl Responder {
    let name = path.into_inner();
    if name.is_empty() {
        return Err(actix_web::error::ErrorBadRequest("Function name is empty"));
    }
    
    let network_client = &data.nc;
    let openfaas_client = &data.ofc;
    let peer_id = &data.peer_id;
    let requests_in_progress = &data.rp;

    // TODO uncomment
    let providers = network_client.lock().await.get_providers(name.clone()).await;
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

    
    let items_result = Arc::new(Mutex::new(Vec::with_capacity(items.capacity())));

    let mut providers_iter = providers.iter();
    let mut actual_item = 0;
    let mut handles = vec![];
    let items_clone = items.clone();
    
    for item in items_clone {
        // TODO remove clone here as used in other scope
        let network_client_clone = Arc::clone(network_client);
        let openfaas_client_clone = Arc::clone(openfaas_client);
        let peer_id_clone = peer_id.clone();
        let requests_in_progress_clone = Arc::clone(requests_in_progress);
        let providers_clone: HashSet<PeerId> = providers.clone();
        let name_clone = name.clone();
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
                        rp_instance.push_req(&p);
                        provider = p;
                    }
                    None => {
                        // Get one from struct
                        provider = rp_instance.get_peer(&providers_clone).unwrap();
                        rp_instance.push_req(&provider);
                    }
                }
            }
            let body = item.as_str().unwrap().to_string().into_bytes();
            let function_response_result;
            if providers_clone.contains(&peer_id_clone) {
                let resp;
                {
                    resp = openfaas_client_clone.lock().await.request_function(&name_clone, Some(body)).await;
                }
                match resp {
                    Ok(resp) => {
                        let resp_body = resp.bytes().await.unwrap().to_vec();
                        function_response_result = resp_body;
                    }
                    Err(err) => {
                        eprintln!("Failed to get response from function {}: {:?}", name_clone, err);
                        function_response_result = "Failed to get response from function".to_string().into_bytes();
                    }
                }
            }
            else {
                let body = body.clone();
                let function_response;
                {
                    function_response = network_client_clone.lock().await.request_function(provider, name_clone, Some(body)).await;
                }
                
                function_response_result = match function_response {
                    Ok(function_response) => function_response,
                    Err(e) => {
                        eprintln!("None of the providers returned file: {:?}", e);
                        "None of the providers returned file".to_string().into_bytes()
                    }
                };
            }
            {
                let mut rp_instance = requests_in_progress_clone.lock().await;
                rp_instance.pop_req(&provider);
            }
            // Lock the Mutex to get access to the vector
            let mut results = shared_result.lock().await;

            // Write to the shared vector
            results[actual_item] = function_response_result;
        });
        // Collect task handles to await them later
        handles.push(handle_call);
        actual_item += 1;
    }

    for handle in handles {
        handle.await.unwrap();
    }

    let results = items_result.lock().await;

    let response = results.to_owned();

    Ok(HttpResponse::Ok().json(response))
}

pub async fn deploy_function(data: web::Data<AppState>, payload: Multipart) -> impl Responder {
    let my_uuid = Uuid::new_v4();
    // add fn- to uuid to create funciton name
    let function_name = "fn-".to_owned() + &my_uuid.to_string();
    let openfaas_client = &data.ofc;
    let network_client = &data.nc;
    
    // Deploy the function to openfaas
    let openfaas_deploy_result = openfaas_client.lock().await.deploy_function(&function_name, payload).await;
    match openfaas_deploy_result {
        Ok(_) => (),
        Err(e) => {
            eprintln!("Failed to deploy function: {:?}", e);
            return Err(actix_web::error::ErrorInternalServerError("Failed to deploy function"));
        }
    }

    // Start providing the function name to the network.    
    network_client.lock().await.start_providing(function_name.clone()).await;
    
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
            return Err(e)
        }
    }

    // Start providing the function name to the network.    
    anounce_provider(network_client, &function_name).await;
    
    Ok(HttpResponse::Ok().body(function_name))
}

async fn deploy_openfaas(openfaas_client: &Arc<Mutex<OpenFaasClient>>, function_name: &String, payload: Multipart) -> Result<(), actix_web::Error> {
    let openfaas_deploy_result = openfaas_client.lock().await.deploy_function(&function_name, payload).await;
    match openfaas_deploy_result {
        Ok(_) => Ok(()),
        Err(e) => {
            eprintln!("Failed to deploy function: {:?}", e);
            return Err(actix_web::error::ErrorInternalServerError("Failed to deploy function"));
        }
    }
}

async fn anounce_provider(network_client: &Arc<Mutex<NetworkClient>>, function_name: &String) {
    network_client.lock().await.start_providing(function_name.clone()).await;
}

async fn function_request(providers: HashSet<PeerId>, peer_id: &PeerId, name: &String, body: &Option<Vec<u8>>, requests_in_progress: &Arc<Mutex<RequestsInProgress>>, openfaas_client: &Arc<Mutex<OpenFaasClient>>, network_client: &Arc<Mutex<NetworkClient>>) -> Result<Vec<u8>, actix_web::Error> {
    let function_response_result;
    if providers.contains(peer_id) {
        let resp = openfaas_client.lock().await.request_function(&name, body.clone()).await;
        match resp {
            Ok(resp) => {
                let resp_body = resp.bytes().await.unwrap().to_vec();
                function_response_result = resp_body;
            }
            Err(err) => {
                eprintln!("Failed to get response from function {}: {:?}", name, err);
                return Err(actix_web::error::ErrorInternalServerError("Failed to read the file"));
            }
        }
    }
    else {
        let requests = providers.into_iter().map(|p| {
            let network_client = network_client.clone();
            let name = name.clone();
            let body = body.clone();
            let requests_in_progress_clone = Arc::clone(requests_in_progress);
            async move { 
                {
                    let mut rp_instance = requests_in_progress_clone.lock().await;
                    rp_instance.push_req(&p);
                }
                let response = network_client.lock().await.request_function(p, name, body).await;
                {
                    let mut rp_instance = requests_in_progress_clone.lock().await;
                    rp_instance.pop_req(&p);
                }
                response
             }.boxed()
        });

        // Await the requests, ignore the remaining once a single one succeeds.
        let function_response = futures::future::select_ok(requests)
            .await;
        
        function_response_result = match function_response {
            Ok(function_response) => function_response.0,
            Err(e) => {
                eprintln!("None of the providers returned file: {:?}", e);
                return Err(actix_web::error::ErrorInternalServerError("None of the providers returned file"));
            }
        };
    }
    Ok(function_response_result)
}
