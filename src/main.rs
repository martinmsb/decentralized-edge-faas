mod network;
mod openfaas;
mod http_server;

use tokio::task::spawn;
use tokio::sync::Mutex;

use futures::StreamExt;
use libp2p::{core::Multiaddr, multiaddr::Protocol};

use std::error::Error;
use std::sync::Arc;

use tracing_subscriber::EnvFilter;
use clap::Parser;

#[actix_web::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let opt = Opt::parse();
    
    let (mut network_client, mut network_events, network_event_loop, peer_id) =
        network::new(opt.secret_key_seed).await?;
    println!("Peer ID: {:?}", peer_id.to_base58());

    // Spawn the network task for it to run in the background.
    spawn(network_event_loop.run());

    // In case a listen address was provided use it, otherwise listen on any
    // address.
    
    network_client
            .start_listening(opt.p2p_listen_address)
            .await
            .expect("Listening fail.");

    // In case the user provided an address of a peer on the CLI, dial it.
    if let Some(addr) = opt.peer {
        let Some(Protocol::P2p(peer_id)) = addr.iter().last() else {
            return Err("Expect peer multiaddr to contain peer ID.".into());
        };
        network_client
            .dial(peer_id, addr)
            .await
            .expect("Dial fail.");
    }
    // Arc allows for multiple ownership and the Mutex ensures safe concurrent access
    let network_client = Arc::new(Mutex::new(network_client));
    
    // Create a new reqwest client
    let openfaas_host = "http://localhost:8080".to_string();
    let openfaas_client = Arc::new(Mutex::new(openfaas::new(openfaas_host, opt.docker_username).await?));
    
    spawn({
    let network_client = Arc::clone(&network_client);
    let openfaas_client = Arc::clone(&openfaas_client);
    async move {
        loop {
            match network_events.next().await {
                // Reply with the content of the file on incoming requests.
                Some(network::Event::InboundRequest { request, channel }) => {
                        // Http request to localhost:8000/functions/name
                        let resp = openfaas_client.lock().await.request_function(&request).await;
                        match resp {
                            Ok(resp) => {
                                let body = resp.bytes().await.unwrap().to_vec();
                                if let Err(err) = network_client.lock().await.respond_function(body, channel).await {
                                    eprintln!("Failed to respond with file: {:?}", err);
                                }
                            }
                            Err(err) => {
                                eprintln!("Failed to send request: {:?}", err);
                            }
                        }
                }
                e => todo!("{:?}", e),
            }
        }
    }
    });

    let app_state = http_server::server::AppState::new(
        Arc::clone(&network_client),
        Arc::clone(&openfaas_client),
                    peer_id.clone()
                );
    
    http_server::server::run_http_server(app_state, opt.http_listen_port).await.expect("HTTP server failed.");
    
    Ok(())
}

#[derive(Parser, Debug)]
#[clap(name = "libp2p file sharing example")]
struct Opt {
    #[clap(long)]
    p2p_listen_address: Multiaddr,
    /// Fixed value to generate deterministic peer ID.
    #[clap(long)]
    secret_key_seed: Option<u8>,

    #[clap(long)]
    peer: Option<Multiaddr>,

    #[clap(long)]
    http_listen_port: u16,

    #[clap(long)]
    docker_username: String,
}
