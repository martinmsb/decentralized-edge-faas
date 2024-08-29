use actix_web::web;

use crate::http_server::handlers::{execute_function, execute_function_manycall, deploy_function, deploy_known_function};

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg
        .service(
            web::scope("/api/v1")
            .service(web::resource("/functions/{name}/executions")
                    .route(web::post().to(execute_function))
            )
            .route("/functions/{name}/executions/manycall", web::post().to(execute_function_manycall))
            .route("/functions/deployments", web::post().to(deploy_function))
            .route("/functions/deployments/{name}", web::put().to(deploy_known_function))
        );
}