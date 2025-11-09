use anyhow::Result;
use askama::Template;
use axum::{
    extract::Form,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use tracing::info;

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate;

impl IntoResponse for IndexTemplate {
    fn into_response(self) -> axum::response::Response {
        match self.render() {
            Ok(html) => Html(html).into_response(),
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Template error: {}", err),
            )
                .into_response(),
        }
    }
}

#[derive(Template)]
#[template(path = "result.html")]
struct ResultTemplate {
    job_id: String,
    message: String,
}

impl IntoResponse for ResultTemplate {
    fn into_response(self) -> axum::response::Response {
        match self.render() {
            Ok(html) => Html(html).into_response(),
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Template error: {}", err),
            )
                .into_response(),
        }
    }
}

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorTemplate {
    error: String,
}

impl IntoResponse for ErrorTemplate {
    fn into_response(self) -> axum::response::Response {
        match self.render() {
            Ok(html) => Html(html).into_response(),
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Template error: {}", err),
            )
                .into_response(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct MathForm {
    a: f64,
    b: f64,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    job_id: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    error: String,
}

async fn index() -> impl IntoResponse {
    IndexTemplate
}

async fn submit_add(Form(form): Form<MathForm>) -> impl IntoResponse {
    submit_job("add", form).await
}

async fn submit_subtract(Form(form): Form<MathForm>) -> impl IntoResponse {
    submit_job("subtract", form).await
}

async fn submit_multiply(Form(form): Form<MathForm>) -> impl IntoResponse {
    submit_job("multiply", form).await
}

async fn submit_divide(Form(form): Form<MathForm>) -> impl IntoResponse {
    submit_job("divide", form).await
}

async fn submit_job(operation: &str, form: MathForm) -> impl IntoResponse {
    let api_url =
        std::env::var("API_SERVICE_URL").unwrap_or_else(|_| "http://api-service:3000".to_string());

    let endpoint = format!("{}/jobs/{}", api_url, operation);

    info!("Submitting {} job: {} and {}", operation, form.a, form.b);

    let client = reqwest::Client::new();
    let response = client
        .post(&endpoint)
        .json(&serde_json::json!({
            "a": form.a,
            "b": form.b,
        }))
        .send()
        .await;

    match response {
        Ok(resp) => {
            if resp.status().is_success() {
                match resp.json::<ApiResponse>().await {
                    Ok(api_resp) => ResultTemplate {
                        job_id: api_resp.job_id,
                        message: api_resp.message,
                    }
                    .into_response(),
                    Err(e) => ErrorTemplate {
                        error: format!("Failed to parse response: {}", e),
                    }
                    .into_response(),
                }
            } else {
                let status = resp.status();
                let error_msg = match resp.json::<ApiError>().await {
                    Ok(err) => err.error,
                    Err(_) => format!("API request failed with status: {}", status),
                };
                ErrorTemplate { error: error_msg }.into_response()
            }
        }
        Err(e) => ErrorTemplate {
            error: format!("Failed to connect to API: {}", e),
        }
        .into_response(),
    }
}

async fn health() -> impl IntoResponse {
    (
        StatusCode::OK,
        axum::Json(serde_json::json!({
            "status": "healthy",
            "service": "frontend-service"
        })),
    )
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8000".to_string());

    info!("Starting frontend service on {}", bind_addr);

    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/submit/add", post(submit_add))
        .route("/submit/subtract", post(submit_subtract))
        .route("/submit/multiply", post(submit_multiply))
        .route("/submit/divide", post(submit_divide));

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
