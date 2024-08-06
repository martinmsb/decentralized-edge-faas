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
        &mut self,
        function_name: &String,
        body: Option<Vec<u8>>
    ) -> Result<Response, Box<dyn Error + Send>> {
        let resp;
        if body == None {
            resp = self.http_client.get(format!("{}/function/{}", self.host, function_name)).send().await;
        } else {
            resp = self.http_client.post(format!("{}/function/{}", self.host, function_name)).body(body.unwrap()).send().await;
        }
        match resp {
            Ok(response) => Ok(response),
            Err(e) => Err(Box::new(e)),
        }
    }
    pub(crate) async fn deploy_function(
        &mut self,
        function_name: &str,
        mut files: Multipart
    ) -> Result<(), Box<dyn Error>>{
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
                eprintln!("Failed to create config file: {:?}", e);
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
                eprintln!("Invalid field name: {}", field_name);
                let _ = self.remove_files();
                return Err(Box::new(io::Error::new(io::ErrorKind::InvalidInput, "Invalid field name")));
            
            }
            
            if field_name == "handler" && handler || field_name == "requirements" && requirements {
                eprintln!("Handler file already uploaded");
                return Err(Box::new(io::Error::new(io::ErrorKind::AlreadyExists, "Handler file already uploaded")));
            }
            let field_extension = if field_name == "handler" { "py" } else { "txt" };
            let mut file = match File::create(format!("openfaas_handler/{}.{}", field_name, field_extension)) {
                Ok(file) => file,
                Err(e) => {
                    eprintln!("Failed to create file: {:?}", e);
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
                        eprintln!("Failed to write to output file: {:?}", e);
                        return Err(Box::new(e));
                    }
                }
            }
            
            
        }
    
        // Deploy the function to openfaas
        let output_result = Command::new("faas-cli").args(["up", "-f", "openfaas_config.yml"]).output();
        match output_result {
            Ok(output) => output,
            Err(e) => {
                eprintln!("Failed to execute command: {:?}", e);
                return Err(Box::new(e));
            }
        };
        
        // Remove files handler.py and requirements.txt from openfaas_handler folder
        self.remove_files().unwrap();
        Ok(())
    }

    fn create_config_file(
        &mut self,
        template_path: &Path,
        output_path: &Path,
        values: &[&str]) -> std::io::Result<()> {
        // Read the template file content
        let template_content = match fs::read_to_string(template_path) {
            Ok(template_content) => template_content,
            Err(e) => {
                eprintln!("Failed to read template file: {:?}", e);
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
                eprintln!("Failed to create output file: {:?}", e);
                return Err(e);
            }
        };
        match file.write_all(result_content.as_bytes()) {
            Ok(_) => (),
            Err(e) => {
                eprintln!("Failed to write to output file: {:?}", e);
                return Err(e);
            }
        }
    
        Ok(())
    }

    fn remove_files(
        &mut self) -> std::io::Result<()> {
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

