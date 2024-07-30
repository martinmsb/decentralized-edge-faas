use actix_web::web;

use crate::http_server::handlers::{get_function, post_function, manual_hello};

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg
        .service(
            web::scope("/api/v1")
                    .route("/function", web::post().to(post_function))
                    .route("/function/{name}", web::get().to(get_function))
        );
}