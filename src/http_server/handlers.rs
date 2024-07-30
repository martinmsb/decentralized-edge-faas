use actix_web::{web, HttpResponse, Responder};
use actix_multipart::Multipart;
use futures::FutureExt;
use uuid::Uuid;

use crate::http_server::server::AppState;

pub async fn get_function(data: web::Data<AppState>, name: web::Path<String>) -> impl Responder {
    let name = name.to_string();
    
    let network_client = &data.nc;
    let openfaas_client = &data.ofc;
    let peer_id = &data.peer_id;

    let file_content_result;
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
    if providers.contains(peer_id) {
        let resp = openfaas_client.lock().await.request_function(&name).await;
        match resp {
            Ok(resp) => {
                let body = resp.bytes().await.unwrap().to_vec();
                file_content_result = body;
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
            async move { network_client.lock().await.request_function(p, name).await }.boxed()
        });

        // Await the requests, ignore the remaining once a single one succeeds.
        let file_content = futures::future::select_ok(requests)
            .await;
        file_content_result = match file_content {
            Ok(file_content_result) => file_content_result.0,
            Err(e) => {
                eprintln!("None of the providers returned file: {:?}", e);
                return Err(actix_web::error::ErrorInternalServerError("None of the providers returned file"));
            }
        };
    }
        
    let s = match String::from_utf8(file_content_result) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Invalid UTF-8 sequence: {:?}", e);
            return Err(actix_web::error::ErrorInternalServerError("Invalid UTF-8 sequence"))
        }
    };

    Ok(HttpResponse::Ok().body(s))

}

pub async fn post_function(data: web::Data<AppState>, payload: Multipart) -> impl Responder {
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

pub async fn manual_hello() -> impl Responder {
    HttpResponse::Ok().body("Hey there!")
}