use actix_multipart::Multipart;
use futures::StreamExt;
//apt install libssl-dev
use reqwest::{Client, Response};
use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;
use regex::Regex;
use uuid::Uuid;

use log::{info, error};

pub(crate) struct OpenFaasClient {
    http_client: Client,
    host: String,
    docker_username: String
}

impl OpenFaasClient {
    pub(crate) fn new(host: String, docker_username: String) -> OpenFaasClient {
        let client = Client::new();
        OpenFaasClient {
                http_client: client,
                host: host,
                docker_username: docker_username
        }
    }
    
    pub(crate) async fn request_function(
        &self,
        function_name: &String,
        method: &String,
        body: Option<Vec<u8>>
    ) -> Result<Response, Box<dyn Error + Send>> {
        let resp;
        match method.as_str() {
            "GET" => {
                resp = self.http_client.get(format!("{}/function/{}", self.host, function_name)).send().await;
            },
            "POST" => {
                if body == None {
                    return Err(Box::new(io::Error::new(io::ErrorKind::InvalidInput, "Body is required for POST method")));
                }
                resp = self.http_client.post(format!("{}/function/{}", self.host, function_name)).body(body.unwrap()).send().await;
            },
            "PUT" => {
                if body == None {
                    return Err(Box::new(io::Error::new(io::ErrorKind::InvalidInput, "Body is required for PUT method")));
                }
                resp = self.http_client.put(format!("{}/function/{}", self.host, function_name)).body(body.unwrap()).send().await;
            },
            "DELETE" => {
                resp = self.http_client.delete(format!("{}/function/{}", self.host, function_name)).send().await;
            },
            "PATCH" => {
                if body == None {
                    return Err(Box::new(io::Error::new(io::ErrorKind::InvalidInput, "Body is required for PATCH method")));
                }
                resp = self.http_client.patch(format!("{}/function/{}", self.host, function_name)).body(body.unwrap()).send().await;
            },
            _ => {
                return Err(Box::new(io::Error::new(io::ErrorKind::InvalidInput, "Not supported method")));
            }
        }
        match resp {
            Ok(response) => Ok(response),
            Err(e) => Err(Box::new(e)),
        }
    }
    pub(crate) async fn deploy_function(
        &self,
        mut files: Multipart,
        optional_function_name: Option<&str>,
    ) -> Result<String, Box<dyn Error>>{
        let my_uuid = Uuid::new_v4();

        let generated_function_name;
        let function_name = match optional_function_name {
            Some(name) => name,
            None => {
                // add fn- to uuid to create funciton name
                generated_function_name = format!("fn-{}", my_uuid.to_string());
                &generated_function_name
            }
        };
        
        let template_path = Path::new("openfaas_config_template.yml");
        let output_path = Path::new("openfaas_config.yml");
        
        let mut values = vec![];
        let docker_username = &self.docker_username.clone();
        values.push(function_name);
        values.push(docker_username);
        values.push(function_name);

        match self.create_config_file(template_path, output_path, &values) {
            Ok(_) => (),
            Err(e) => {
                error!("Failed to create config file: {:?}", e);
                return Err(Box::new(e));
            }
        }
        
        // Store multipart files handler.py and requirements.txt inside openfaas_handler folder
        let mut handler = false;
        let mut requirements = false;
        while let Some(field) = files.next().await {
            let mut field = match field {
                Ok(field) => field,
                Err(e) => return Err(Box::new(e)),
            };
            
            let field_name = field.name().to_owned();

            if field_name != "handler" && field_name != "requirements" {
                error!("Invalid field name: {}", field_name);
                let _ = self.remove_files();
                return Err(Box::new(io::Error::new(io::ErrorKind::InvalidInput, "Invalid field name")));
            
            }
            
            if field_name == "handler" && handler || field_name == "requirements" && requirements {
                error!("Handler file already uploaded");
                return Err(Box::new(io::Error::new(io::ErrorKind::AlreadyExists, "Handler file already uploaded")));
            }
            let field_extension = if field_name == "handler" { "py" } else { "txt" };
            let mut file = match File::create(format!("openfaas_handler/{}.{}", field_name, field_extension)) {
                Ok(file) => file,
                Err(e) => {
                    error!("Failed to create file: {:?}", e);
                    return Err(Box::new(e));
                }
            };
            // Write the file content to the file
            while let Some(chunk) = field.next().await {
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(e) => return Err(Box::new(e)),
                };

                match file.write_all(&chunk) {
                    Ok(_) => if field_name == "handler" {handler = true} else {requirements = true},
                    Err(e) => {
                        error!("Failed to write to output file: {:?}", e);
                        return Err(Box::new(e));
                    }
                }
            }
            
            
        }
    
        // Deploy the function to openfaas
        let output_result = Command::new("faas-cli").args(["up", "-f", "openfaas_config.yml"]).output();
        match output_result {
            Ok(output) => {
                if output.status.success() {
                    info!("Function deployed successfully");
                } else {
                    error!("Failed to deploy function: {:?}", output);
                    self.remove_files().unwrap();
                    return Err(Box::new(io::Error::new(io::ErrorKind::Other, "Failed to deploy function. Check handler and requirements files")));
                }
            },
            Err(e) => {
                error!("Failed to execute command: {:?}", e);
                return Err(Box::new(e));
            }
        };
        
        // Remove files handler.py and requirements.txt from openfaas_handler folder
        self.remove_files().unwrap();
        Ok(function_name.to_owned())
    }

    fn create_config_file(
        &self,
        template_path: &Path,
        output_path: &Path,
        values: &[&str]) -> std::io::Result<()> {
        // Read the template file content
        let template_content = match fs::read_to_string(template_path) {
            Ok(template_content) => template_content,
            Err(e) => {
                error!("Failed to read template file: {:?}", e);
                return Err(e);
            }
        };
        // Create a regex pattern to match the placeholders
        let re = Regex::new(r"\{\}").unwrap();
    
        // Replace all placeholders with values from the list
        let mut result_content = template_content.clone();
        for value in values {
            result_content = re.replace(&result_content, *value).to_string();
        }
    
        // Write the updated content to the output file
        let mut file = match File::create(output_path) {
            Ok(file) => file,
            Err(e) => {
                error!("Failed to create output file: {:?}", e);
                return Err(e);
            }
        };
        match file.write_all(result_content.as_bytes()) {
            Ok(_) => (),
            Err(e) => {
                error!("Failed to write to output file: {:?}", e);
                return Err(e);
            }
        }
    
        Ok(())
    }

    fn remove_files(
        &self) -> std::io::Result<()> {
        let dir = fs::read_dir("openfaas_handler").unwrap();
        for entry in dir {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_file() && path.file_name().unwrap() != "tox.ini"{
                fs::remove_file(path).unwrap();
            }
        }
        // Remove openfaas_config.yml
        fs::remove_file("openfaas_config.yml").unwrap();
        Ok(())
    }
}

