use tokio_tungstenite::tungstenite::Message as TungsteniteWsMessage;
use axum::extract::ws::Message as AxumWsMessage;

pub fn axum_to_tungstein(msg: AxumWsMessage) -> TungsteniteWsMessage {
    match msg {
        AxumWsMessage::Text(text) => TungsteniteWsMessage::Text(text),
        AxumWsMessage::Binary(data) => TungsteniteWsMessage::Binary(data),
        AxumWsMessage::Ping(ping) => TungsteniteWsMessage::Ping(ping),
        AxumWsMessage::Pong(pong) => TungsteniteWsMessage::Pong(pong),
        AxumWsMessage::Close(_) => TungsteniteWsMessage::Close(None),
    }
}

pub fn tungstein_to_axum(msg: TungsteniteWsMessage) -> Option<AxumWsMessage> {
    match msg {
        TungsteniteWsMessage::Text(text) => AxumWsMessage::Text(text).into(),
        TungsteniteWsMessage::Binary(data) => AxumWsMessage::Binary(data).into(),
        TungsteniteWsMessage::Ping(data) => AxumWsMessage::Ping(data).into(),
        TungsteniteWsMessage::Pong(data) => AxumWsMessage::Pong(data).into(),
        TungsteniteWsMessage::Close(_) => AxumWsMessage::Close(None).into(),
        TungsteniteWsMessage::Frame(_) => None,
    }
}