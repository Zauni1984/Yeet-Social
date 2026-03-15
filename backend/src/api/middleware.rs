use axum::{
    async_trait,
    extract::{FromRequestParts, State},
    http::{request::Parts, StatusCode, HeaderMap},
    RequestPartsExt,
};
use axum_extra::{
    headers::{Authorization, authorization::Bearer},
    TypedHeader,
};
use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};
use uuid::Uuid;
use crate::AppState;
use crate::api::auth::Claims;

/// Authenticated user extracted from JWT Bearer token.
/// Add `auth: AuthUser` as a parameter to any handler to require auth.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: Uuid,
    pub wallet: String,
}

#[async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // Extract Bearer token from Authorization header
        let TypedHeader(Authorization(bearer)) = parts
            .extract::<TypedHeader<Authorization<Bearer>>>()
            .await
            .map_err(|_| (StatusCode::UNAUTHORIZED, "Missing Authorization header"))?;

        // Decode and validate JWT
        let token_data = decode::<Claims>(
            bearer.token(),
            &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
            &Validation::new(Algorithm::HS256),
        )
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or expired token"))?;

        let user_id = token_data.claims.sub
            .parse::<Uuid>()
            .map_err(|_| (StatusCode::UNAUTHORIZED, "Malformed token subject"))?;

        Ok(AuthUser {
            user_id,
            wallet: token_data.claims.wallet,
        })
    }
}

/// Optional auth — returns None if no valid token present.
/// Use for endpoints that behave differently for logged-in users.
pub struct OptionalAuthUser(pub Option<AuthUser>);

#[async_trait]
impl FromRequestParts<AppState> for OptionalAuthUser {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(OptionalAuthUser(
            AuthUser::from_request_parts(parts, state).await.ok()
        ))
    }
}
