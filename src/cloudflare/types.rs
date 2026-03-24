use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct CfResponse<T> {
    pub result: T,
    pub success: bool,
}

#[derive(Debug, Deserialize)]
pub struct CfListResponse<T> {
    pub result: Vec<T>,
    pub success: bool,
}

#[derive(Debug, Deserialize)]
pub struct Zone {
    pub id: String,
    pub name: String,
    pub account: Account,
}

#[derive(Debug, Deserialize)]
pub struct Account {
    pub id: String,
}

#[derive(Debug, Serialize)]
pub struct CreateTunnelRequest {
    pub name: String,
    pub tunnel_secret: String,
    pub config_src: String,
}

#[derive(Debug, Deserialize)]
pub struct Tunnel {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct CreateDnsRecordRequest {
    #[serde(rename = "type")]
    pub record_type: String,
    pub name: String,
    pub content: String,
    pub proxied: bool,
    pub comment: String,
    pub ttl: u32,
}

#[derive(Debug, Deserialize)]
pub struct DnsRecord {
    pub id: String,
    pub name: String,
    pub content: String,
    #[serde(rename = "type")]
    pub record_type: String,
}
