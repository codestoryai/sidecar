use axum::{
    routing::{post, get},
    Router,
    Json,
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use jsonwebtoken::{encode, decode, Header, EncodingKey, DecodingKey, Validation, errors::Error as JwtError};
use time::{Duration, OffsetDateTime};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    sub: String,
    exp: i64,
    iat: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenRequest {
    username: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenResponse {
    token: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RefreshRequest {
    token: String,
}

const JWT_SECRET: &[u8] = b"sidecar_secret_key";
const TOKEN_DURATION_HOURS: i64 = 24;

pub fn router() -> Router {
    Router::new()
        .route("/auth/token", post(generate_token))
        .route("/auth/validate", post(validate_token))
        .route("/auth/refresh", post(refresh_token))
}

async fn generate_token(Json(req): Json<TokenRequest>) -> Result<Json<TokenResponse>, StatusCode> {
    let now = OffsetDateTime::now_utc();
    let exp = now + Duration::hours(TOKEN_DURATION_HOURS);
    
    let claims = Claims {
        sub: req.username,
        exp: exp.unix_timestamp(),
        iat: now.unix_timestamp(),
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(JWT_SECRET),
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(TokenResponse { token }))
}

async fn validate_token(Json(req): Json<RefreshRequest>) -> Result<StatusCode, StatusCode> {
    match decode::<Claims>(
        &req.token,
        &DecodingKey::from_secret(JWT_SECRET),
        &Validation::default(),
    ) {
        Ok(_) => Ok(StatusCode::OK),
        Err(JwtError::ExpiredSignature) => Err(StatusCode::UNAUTHORIZED),
        Err(_) => Err(StatusCode::BAD_REQUEST),
    }
}

async fn refresh_token(Json(req): Json<RefreshRequest>) -> Result<Json<TokenResponse>, StatusCode> {
    let token_data = decode::<Claims>(
        &req.token,
        &DecodingKey::from_secret(JWT_SECRET),
        &Validation::default(),
    ).map_err(|_| StatusCode::UNAUTHORIZED)?;

    let now = OffsetDateTime::now_utc();
    let exp = now + Duration::hours(TOKEN_DURATION_HOURS);
    
    let new_claims = Claims {
        sub: token_data.claims.sub,
        exp: exp.unix_timestamp(),
        iat: now.unix_timestamp(),
    };

    let new_token = encode(
        &Header::default(),
        &new_claims,
        &EncodingKey::from_secret(JWT_SECRET),
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(TokenResponse { token: new_token }))
}