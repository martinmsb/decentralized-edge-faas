use crate::network::NetworkClient;
use crate::openfaas::OpenFaasClient;
use crate::data_structures::RequestsInProgress;
use crate::model::OpenFaaSResponse;
use crate::model::detect_and_parse_body;
use crate::model::serialize_body;

use tokio::spawn;
use tokio::sync::Mutex;
use std::sync::Arc;
use futures::FutureExt;

use std::io;
use std::collections::HashSet;
use std::error::Error;
use libp2p::PeerId;

use actix_multipart::Multipart;
use serde_json::Value;
use serde_json::json;

use log::{info, error};

pub(crate) struct FunctionsService {
    pub(crate) nc: Arc<NetworkClient>,
    pub(crate) ofc: Arc<OpenFaasClient>,
    pub(crate) rp: Arc<Mutex<RequestsInProgress>>,
    pub(crate) peer_id: libp2p::PeerId,
}

impl FunctionsService {
    pub(crate) fn new(
        nc: Arc<NetworkClient>,
        ofc: Arc<OpenFaasClient>,
        rp: Arc<Mutex<RequestsInProgress>>,
        peer_id: PeerId,
    ) -> Self {
        Self { nc, ofc, rp, peer_id }
    }

    pub(crate) async fn execute_function(&self, mut name: String, method: &String, body: Option<Vec<u8>>, path_and_query_field: &Option<String>) -> Result<OpenFaaSResponse, Box<dyn Error>>{
        let network_client = &self.nc;
        let openfaas_client = &self.ofc;
        let requests_in_progress = &self.rp;
        let peer_id = &self.peer_id;

        let function_response_result;
        // Locate all nodes providing the function.
        let providers = network_client.get_providers(name.clone()).await;
        info!("providers: {:?}", providers);
        if providers.is_empty() {
            return Err(Box::new(io::Error::new(io::ErrorKind::NotFound, "No providers found for function")));
        }
        
        if path_and_query_field.is_some() {
            name = name + path_and_query_field.as_ref().unwrap();
        }

        // Request the content of the file from each node.
        info!("providers: {:?}", providers);
        // Check if providers length is 1 and then check if it is the same as the peer_id
        // If it is, then return the file content
        match self.function_request(providers, peer_id, &name, &method, &body, requests_in_progress, openfaas_client, network_client).await
        {
            Ok(resp) => function_response_result = resp,
            Err(e) => return Err(e)
        }
        
        return Ok(function_response_result)
    }

    pub(crate) async fn execute_function_manycall(&self, name: &String, items: Vec<Value>) -> Result<Vec<Value>, Box<dyn Error>> {
        let network_client = &self.nc;
        let openfaas_client = &self.ofc;
        let requests_in_progress = &self.rp;
        let peer_id = &self.peer_id;

        let providers = network_client.get_providers(name.clone()).await;
        info!("providers: {:?}", providers);

        if providers.is_empty() {
            return Err(Box::new(io::Error::new(io::ErrorKind::NotFound,"Could not find provider")));
        }
            
        let items_result: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(vec![serde_json::Value::String("".to_string()); items.len()]));

        let providers_not_used = Arc::new(Mutex::new(providers.clone()));
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
            let shared_providers_not_used    = Arc::clone(&providers_not_used);

            // Spawn a new asynchronous task
            let handle_call = spawn(async move {
                let provider: PeerId;
                {
                    // In the closure, lock the Mutex to get access to the Requests in Progress to unlock it after the closure
                    let mut rp_instance = requests_in_progress_clone.lock().await;
                    let provider_nip;
                    {
                        let pnu_lock = shared_providers_not_used.lock().await;
                        if pnu_lock.is_empty() {
                            provider_nip = None;
                        } else {
                            provider_nip = rp_instance.find_not_in_progress_peer(&providers_clone);
                        }
                    }

                    let mut first_use = false;
                    match provider_nip {
                        Some(p) => {
                            let mut pnu_lock = shared_providers_not_used.lock().await;
                            if pnu_lock.contains(&p) {
                                first_use = true;
                                pnu_lock.remove(&p);
                            }
                            rp_instance.push_req(&p, first_use);
                            provider = p;
                        }
                        None => {
                            // Get one from struct
                            let mut pnu_lock = shared_providers_not_used.lock().await;
                            provider = rp_instance.get_peer(&providers_clone).unwrap();
                            if pnu_lock.contains(&provider) {
                                first_use = true;
                                pnu_lock.remove(&provider);
                            }
                            rp_instance.push_req(&provider, first_use);
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
                            error!("Failed to get response from function {}: {:?}", name_clone, err);
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
                            error!("Response from provider failed: {:?}", e);
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

        Ok(response)
    }

    pub(crate) async fn deploy_function(&self, payload: Multipart) -> Result<String, Box<dyn Error>> {
        let network_client = &self.nc;
        // Deploy the function to openfaas
        let deployment_result = self.deploy_openfaas(payload, None).await;
        let function_name;
        match deployment_result {
            Ok(f_n) => function_name = f_n,
            Err(e) => {
                error!("Failed to deploy function: {:?}", e);
                return Err(Box::new(io::Error::new(io::ErrorKind::Other, "Failed to deploy function")));
            }
        }

        // Start providing the function name to the network.    
        network_client.start_providing(function_name.clone()).await;

        Ok(function_name)
    }

    pub(crate) async fn deploy_known_function(&self, function_name: &String, payload: Multipart) -> Result<(), Box<dyn Error>> {
        let network_client = &self.nc;

        // Deploy the function to openfaas
        let deployment_result = self.deploy_openfaas(payload, Some(function_name)).await;
        
        match deployment_result {
            Ok(_) => (),
            Err(e) => {
                error!("Failed to deploy function: {:?}", e);
                return Err(Box::new(io::Error::new(io::ErrorKind::Other, "Failed to deploy function")));
            }
        }

        // Start providing the function name to the network.    
        network_client.start_providing(function_name.clone()).await;

        Ok(())
    }

    async fn function_request(&self, providers: HashSet<PeerId>, peer_id: &PeerId, name: &String, method: &String, body: &Option<Vec<u8>>, requests_in_progress: &Arc<Mutex<RequestsInProgress>>, openfaas_client: &Arc<OpenFaasClient>, network_client: &Arc<NetworkClient>) -> Result<OpenFaaSResponse, Box<dyn Error>> {
        let function_response_status;
        let function_response_body;
        
        if providers.contains(peer_id) {
            let requests_in_progress_clone = Arc::clone(requests_in_progress);
            {
                let mut rp_instance = requests_in_progress_clone.lock().await;
                rp_instance.push_req(peer_id, false);
            }
            let resp = openfaas_client.request_function(&name, &method, body.clone()).await;
            {
                let mut rp_instance = requests_in_progress_clone.lock().await;
                rp_instance.pop_req(peer_id, false);
            }
            match resp {
                Ok(resp) => {
                    function_response_status = resp.status().as_u16();
                    let resp_body = resp.bytes().await.unwrap().to_vec();
                    function_response_body = resp_body;
                }
                Err(err) => {
                    error!("Failed to get response from function {}: {:?}", name, err);
                    return Err(Box::new(io::Error::new(io::ErrorKind::Other, "Failed to get response from function")));
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
                    error!("None of the providers responded: {:?}", e);
                    return Err(Box::new(io::Error::new(io::ErrorKind::NotFound,"None of the providers responded")));
                }
            };
        }
        info!("Response received in service, body: {:?}", function_response_body);
                
        let openfaas_response = OpenFaaSResponse {
            status: function_response_status,
            body: function_response_body
        };
        Ok(openfaas_response)
    }

    async fn deploy_openfaas(&self, payload: Multipart, function_name: Option<&str>) -> Result<String, actix_web::Error> {
        let openfaas_client = &self.ofc;
        let deployment_result = openfaas_client.deploy_function(payload, function_name).await;
        let function_name;
        match deployment_result {
            Ok(fun_name) => function_name = fun_name,
            Err(e) => {
                error!("Failed to deploy function: {:?}", e);
                return Err(actix_web::error::ErrorInternalServerError("Failed to deploy function"));
            }
        }
        Ok(function_name)
    }

}

