use crate::auth::session::{extract_session_token, verify_signed_token};
use crate::db::users::get_session_with_user;
use crate::error::AppError;
use crate::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::HeaderMap,
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct WsAuthQuery {
    pub token: Option<String>,
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<WsAuthQuery>,
) -> Result<Response, AppError> {
    // 1. Authenticate token from query param or header/cookie
    let token = query
        .token
        .or_else(|| extract_session_token(&headers))
        .ok_or_else(|| AppError::Unauthorized("WebSocket requires authentication".into()))?;

    if verify_signed_token(&token, &state.config.session_secret).is_none() {
        return Err(AppError::Unauthorized("Invalid WebSocket token".into()));
    }

    let conn = state.pool.get()?;
    let session_res = get_session_with_user(&conn, &token)?;
    let (_session, user) =
        session_res.ok_or_else(|| AppError::Unauthorized("Session expired or invalid".into()))?;

    tracing::info!("WebSocket client connected: {}", user.email);

    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state, user.id)))
}

async fn handle_socket(socket: WebSocket, state: AppState, user_id: String) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.hub.subscribe();

    // Spawn a task to forward broadcast messages to this client
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            // Forward event to WebSocket client
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Task to read incoming client messages (e.g. ping)
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Close(_) => break,
                Message::Ping(_) => (),
                Message::Text(t) if t.contains("ping") => {
                    // Responding or acknowledging ping if needed
                }
                _ => (),
            }
        }
    });

    // If either task finishes, abort the other
    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    };

    tracing::info!("WebSocket client disconnected for user {}", user_id);
}
