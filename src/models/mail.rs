/*
 * FDK Mail Sender Service
 *
 * API for sending mail
 *
 * The version of the OpenAPI document: 0.1.0
 * 
 * Generated by: https://openapi-generator.tech
 */




#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Mail {
    #[serde(rename = "from")]
    pub from: String,
    #[serde(rename = "to")]
    pub to: String,
    #[serde(rename = "subject")]
    pub subject: String,
    #[serde(rename = "body")]
    pub body: String,
}

impl Mail {
    pub fn new(from: String, to: String, subject: String, body: String) -> Mail {
        Mail {
            from,
            to,
            subject,
            body,
        }
    }
}


