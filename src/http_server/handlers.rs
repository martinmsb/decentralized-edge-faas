use actix_web::http::StatusCode;
use actix_web::{web, HttpResponse, Responder};
use actix_multipart::Multipart;
use reqwest::Method;

use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

use log::{info, error};

use crate::http_server::server::AppState;
use crate::model::serialize_body;
use crate::model::detect_and_parse_body;

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

pub async fn execute_function(data: web::Data<AppState>, path: web::Path<String>, req_body: web::Json<AnycallBody>) -> impl Responder {
    let name = path.into_inner();
    if name.is_empty() {
        return Err(actix_web::error::ErrorBadRequest("Function name is empty"));
    }
    
    let functions_service = &data.fs;

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

    let function_response_result = functions_service.execute_function(name, method, body, path_and_query_field).await;

    match function_response_result {
        Ok(function_response_result) => {
            let s = serialize_body(detect_and_parse_body(function_response_result.body)).to_string();
            info!("Response received in handler, body: {:?}", s);
            return Ok(HttpResponse::build(StatusCode::from_u16(function_response_result.status).unwrap()).body(s));
        },
        Err(e) => return Err(actix_web::error::ErrorInternalServerError(e))
    }
}

pub async fn execute_function_manycall(data: web::Data<AppState>, path: web::Path<String>, req_body: web::Json<ManycallBody>) -> impl Responder {
    let name = path.into_inner();
    if name.is_empty() {
        return Err(actix_web::error::ErrorBadRequest("Function name is empty"));
    }
        
    let functions_service = &data.fs;

    let items = match req_body.items {
        Value::Array(ref array) => {
            // Handle the case where the item is an array
            array.to_owned()
        }
        _ => return Ok(HttpResponse::BadRequest().body("Invalid request body. Field items must be an array")),
    };

    let response = functions_service.execute_function_manycall(&name, items).await;

    let response = match response {
        Ok(response) => response,
        Err(e) => {
            error!("Failed to execute manycall: {:?}", e);
            return Err(actix_web::error::ErrorInternalServerError("Failed to execute manycall"));
        }
    };
    let body = json!({
        "results": response
    });

    Ok(HttpResponse::Ok().json(body))
}

pub async fn deploy_function(data: web::Data<AppState>, payload: Multipart) -> impl Responder {
    let functions_service = &data.fs;

    let function_name = functions_service.deploy_function(payload).await;
    
    match function_name {
        Ok(function_name) => return Ok(HttpResponse::Ok().body(function_name)),
        Err(e) => {
            error!("Failed to deploy function: {:?}", e);
            return Err(actix_web::error::ErrorInternalServerError("Failed to deploy function"));
        }
    }
}

pub async fn deploy_known_function(data: web::Data<AppState>, payload: Multipart, path: web::Path<String>) -> impl Responder {
    let function_name = path.into_inner();
    if function_name.is_empty() {
        return Err(actix_web::error::ErrorBadRequest("Function name is empty"));
    }
    
    let functions_service = &data.fs;
    
    let deployment_result = functions_service.deploy_known_function(&function_name, payload).await;

    match deployment_result {
        Ok(_) => return Ok(HttpResponse::Ok().body(function_name.clone())),
        Err(e) => {
            error!("Failed to deploy function: {:?}", e);
            return Err(actix_web::error::ErrorInternalServerError("Failed to deploy function"));
        }
    }
}


