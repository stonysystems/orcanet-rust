use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct FileRequest {
    pub(crate) file_hash: String,
    pub(crate) requester_id: String, // Bitcoin address
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FileResponse {
    file_name: String,
    file_hash: String,
    server_id: String, // Bitcoin address
    // cost_per_kb: f64, // Cost in BTC
    cost: f64, // cost in BTC - TODO: Change later
    file_content_b64: String,
}

pub enum Detail {
    FileRequest(FileRequest),
    FileResponse(FileResponse),
    // HTTPProxyRequest,
    // HTTPProxyResponse,
}


pub struct RequestHandler;

impl RequestHandler {
    pub fn handle_request(request: String) -> Option<String> {
        let parsed_json: serde_json::Value = serde_json::from_str(request.as_str()).expect("Invalid json");
        let request_type = parsed_json.get("request_type").unwrap().as_str();
        let detail_value = parsed_json.get("detail").unwrap();

        let detail = match request_type {
            Some("file_request") => {
                Detail::FileRequest(serde_json::from_value(detail_value.clone())
                    .expect("Wrong format for file request!"))
            }
            Some("file_response") => {
                Detail::FileResponse(serde_json::from_value(detail_value.clone())
                    .expect("Wrong format for file response!"))
            }
            _ => {
                panic!("request_type not present or invalid")
            }
        };

        return match detail {
            Detail::FileRequest(file_request) => {
                Some(Self::handle_file_request(file_request))
            }
            Detail::FileResponse(file_response) => {
                Self::handle_file_response(file_response);
                None
            }
        };
    }

    pub fn handle_file_request(file_request: FileRequest) -> String {
        println!("Handled file_request {:?}", file_request);
        return String::from("wow");
    }

    pub fn handle_file_response(file_response: FileResponse) {
        println!("Handled file_response {:?}", file_response);
    }
}