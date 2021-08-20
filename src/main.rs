use axum::body::{box_body, Body, BoxBody, Bytes};
use axum::http::{Response, StatusCode};
use deadpool_postgres::Pool;
use figment::providers::{Format, Toml};
use figment::Figment;
use http::Request;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;
use tower::util::MapResponseLayer;
use tower::{filter::AsyncFilterLayer, util::AndThenLayer, BoxError, ServiceBuilder};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::debug;

use crate::config::Config;
use axum::{handler::get, handler::post, AddExtensionLayer, Router};

mod config;

#[tokio::main]
async fn main() {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var(
            "RUST_LOG",
            "feiyuan_web=debug,tower_http=debug,tokio_postgres=debug",
        )
    }

    tracing_subscriber::fmt::init();

    let config: Config = Figment::new()
        .merge(Toml::file("Config.toml"))
        .extract()
        .unwrap();

    let pg_config = tokio_postgres::Config::from_str(&config.global.db_url).unwrap();
    let mgr = deadpool_postgres::Manager::from_config(
        pg_config,
        tokio_postgres::NoTls,
        deadpool_postgres::ManagerConfig {
            recycling_method: deadpool_postgres::RecyclingMethod::Fast,
        },
    );
    let pool = Pool::new(mgr, config.global.pool_size);

    let app = Router::new()
        .nest(
            "/",
            axum::service::get(ServeDir::new("./publish").append_index_html_on_directories(true))
                .handle_error(|error: std::io::Error| {
                    Ok::<_, std::convert::Infallible>((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Unhandled internal error: {}", error),
                    ))
                }),
        )
        .nest(
            "/upload",
            axum::service::get(ServeDir::new("./upload").append_index_html_on_directories(false))
                .handle_error(|error: std::io::Error| {
                    tracing::debug!("{:?}", error);
                    Ok::<_, std::convert::Infallible>((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "read file error".to_string(),
                    ))
                }),
        )
        .nest(
            "/api",
            Router::new()
                .route("/pay/pay_params", post(handle))
                .route("/pay/transfer_info", post(handle))
                .route("/upload/image", post(handle))
                .nest(
                    "/user",
                    Router::new()
                        .route("/list", get(handle))
                        .route("/create", post(handle))
                        .route("/login", post(handle))
                        .route("/info", get(handle))
                        .route("/update_password", get(handle))
                        .route("/money", get(handle)),
                )
                .nest(
                    "/order",
                    Router::new()
                        .route("/create", post(handle))
                        .route("/all", get(handle))
                        .route("/list", get(handle)),
                )
                .nest(
                    "/product",
                    Router::new()
                        .route("/list", get(handle))
                        .route("/home", get(handle))
                        .route("/create", post(handle))
                        .route("/update/:id", post(handle))
                        .route("/delete", post(handle))
                        .route("/detail", get(handle))
                        .route("/earnings/create", post(handle))
                        .route("/earnings/delete", post(handle))
                        .route("/earnings/find", get(handle))
                        .boxed(),
                )
                .layer(AsyncFilterLayer::new(map_request))
                .layer(AndThenLayer::new(map_response))
                .handle_error(|error: BoxError| {
                    if error.is::<tower::timeout::error::Elapsed>() {
                        Ok::<_, Infallible>((
                            StatusCode::REQUEST_TIMEOUT,
                            "request took too long".to_string(),
                        ))
                    } else {
                        tracing::debug!("{:?}", error);
                        Ok::<_, Infallible>((
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Unhandled internal error".to_string(),
                        ))
                    }
                }),
        )
        .layer(MapResponseLayer::new(map_404))
        .layer(
            ServiceBuilder::new()
                .timeout(Duration::from_secs(15))
                .layer(TraceLayer::new_for_http())
                .layer(AddExtensionLayer::new(pool))
                .into_inner(),
        );
    // .check_infallible();
    let addr: SocketAddr = config.global.address.parse::<SocketAddr>().unwrap();
    debug!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

fn map_404(response: Response<BoxBody>) -> Response<BoxBody> {
    if response.status() == StatusCode::NOT_FOUND
        || response.status() == StatusCode::METHOD_NOT_ALLOWED
    {
        let json = r#"{"code": 0, "msg": "not found"}"#;
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("Content-Type", "application/json")
            .body(box_body(Body::from(json)))
            .unwrap();
    }

    response
}

async fn map_request(req: Request<Body>) -> Result<Request<Body>, BoxError> {
    let (parts, body) = req.into_parts();
    let bytes = buffer_and_print("request", body).await?;
    let req = Request::from_parts(parts, Body::from(bytes));
    Ok(req)
}

async fn map_response(res: Response<BoxBody>) -> Result<Response<Body>, BoxError> {
    let (parts, body) = res.into_parts();
    let bytes = buffer_and_print("response", body).await?;
    let res = Response::from_parts(parts, Body::from(bytes));
    Ok(res)
}

async fn buffer_and_print<B>(direction: &str, body: B) -> Result<Bytes, BoxError>
where
    B: axum::body::HttpBody<Data = Bytes>,
    B::Error: Into<BoxError>,
{
    let bytes = hyper::body::to_bytes(body).await.map_err(Into::into)?;
    if let Ok(body) = std::str::from_utf8(&bytes) {
        tracing::debug!("{} body = {:?}", direction, body);
    }
    Ok(bytes)
}

async fn handle() -> &'static str {
    return "Hello,World";
}
