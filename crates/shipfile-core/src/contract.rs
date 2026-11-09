//! The environment-variable contract (ADR-0006, PLAN §5.2).
//!
//! [`Model::contract`] is the only place where `uses` edges become variable names, local
//! values, AWS values and IAM actions. Compose, CloudFormation and the bindings all read it.

use serde::Serialize;

use crate::model::{Database, Model, ResourceKind};

/// ElasticMQ's host name and port in the local stack.
pub const LOCAL_SQS_ENDPOINT: &str = "http://elasticmq:9324";
/// DynamoDB Local's host name and port in the local stack.
pub const LOCAL_DYNAMODB_ENDPOINT: &str = "http://dynamodb:8000";
/// Host name of the local Postgres container.
pub const LOCAL_POSTGRES_HOST: &str = "postgres";
/// Postgres port, locally and on RDS.
pub const POSTGRES_PORT: u16 = 5432;
/// Postgres user, locally and as the RDS master user.
pub const POSTGRES_USER: &str = "shipfile";
/// Region used locally when the spec has none.
pub const LOCAL_DEFAULT_REGION: &str = "us-east-1";

/// Everything one service needs to reach what it `uses`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnvContract {
    pub service: String,
    /// Sorted by name.
    pub vars: Vec<EnvVar>,
    /// IAM permissions, sorted by role then resource.
    pub iam: Vec<IamGrant>,
    /// Databases whose security group must accept this service.
    pub network: Vec<String>,
}

/// One environment variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnvVar {
    pub name: String,
    /// What it comes from: `queues.orders-queue`, `secrets.STRIPE_KEY`, `env`, or `platform`.
    pub source: String,
    pub local: LocalValue,
    /// `None` for local-only variables (emulator endpoints, dummy credentials).
    pub aws: Option<AwsValue>,
    /// Never print the value.
    pub secret: bool,
}

/// A variable's value in the local Docker stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LocalValue {
    Literal { value: String },
    /// Read from `.env.local`.
    DotEnv { key: String },
}

/// A variable's value on AWS, as a CloudFormation-level reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AwsValue {
    Literal {
        value: String,
    },
    /// The `AWS::Region` pseudo parameter.
    Region,
    /// `Ref` of the resource's main template resource (SQS → queue URL, DynamoDB → table name).
    Ref {
        resource: String,
    },
    /// `Fn::GetAtt` on the resource's main template resource.
    GetAtt {
        resource: String,
        attribute: String,
    },
    /// The RDS-managed master secret, injected through ECS `secrets`.
    ManagedSecret {
        resource: String,
        json_key: String,
    },
    /// An SSM `SecureString` parameter, injected through ECS `secrets`.
    Ssm {
        parameter: String,
    },
}

/// Which ECS role a permission belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Used by the application code.
    Task,
    /// Used by ECS to start the task (pull images, read secrets).
    Execution,
}

/// Actions a role may perform on one resource.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct IamGrant {
    pub role: Role,
    /// `queues.orders-queue`, `databases.main-db`, or `secrets` for the service's SSM parameters.
    pub resource: String,
    pub actions: Vec<&'static str>,
}

const SQS_ACTIONS: &[&str] = &[
    "sqs:ChangeMessageVisibility",
    "sqs:DeleteMessage",
    "sqs:GetQueueAttributes",
    "sqs:GetQueueUrl",
    "sqs:ReceiveMessage",
    "sqs:SendMessage",
];
const DYNAMODB_ACTIONS: &[&str] = &[
    "dynamodb:BatchGetItem",
    "dynamodb:BatchWriteItem",
    "dynamodb:ConditionCheckItem",
    "dynamodb:DeleteItem",
    "dynamodb:DescribeTable",
    "dynamodb:GetItem",
    "dynamodb:PutItem",
    "dynamodb:Query",
    "dynamodb:Scan",
    "dynamodb:UpdateItem",
];

/// `orders-queue` → `ORDERS_QUEUE`.
pub fn var_prefix(resource: &str) -> String {
    resource.to_ascii_uppercase().replace('-', "_")
}

/// `main-db` → `main_db` (Postgres database names cannot contain `-`).
pub fn db_name(resource: &str) -> String {
    resource.replace('-', "_")
}

/// The variable names a `uses` edge produces.
pub fn resource_var_names(kind: ResourceKind, resource: &str) -> Vec<String> {
    let p = var_prefix(resource);
    match kind {
        ResourceKind::Queue => vec![format!("{p}_QUEUE_URL")],
        ResourceKind::Postgres => ["HOST", "PORT", "NAME", "USER", "PASSWORD"].iter().map(|s| format!("{p}_{s}")).collect(),
        ResourceKind::Dynamodb => vec![format!("{p}_TABLE")],
    }
}

/// Variables every service gets from the platform. Users may not set these themselves.
pub const PLATFORM_VARS: &[&str] =
    &["AWS_ACCESS_KEY_ID", "AWS_ENDPOINT_URL_DYNAMODB", "AWS_ENDPOINT_URL_SQS", "AWS_REGION", "AWS_SECRET_ACCESS_KEY"];

impl Model {
    /// The env contract for one service. Returns `None` if the service does not exist.
    pub fn contract(&self, service: &str) -> Option<EnvContract> {
        let svc = self.services.get(service)?;
        let mut vars = Vec::new();
        let mut iam = Vec::new();
        let mut network = Vec::new();
        let lit = |v: &str| LocalValue::Literal { value: v.to_owned() };
        let env_name = self.app.env.as_deref().unwrap_or("default");

        for u in self.uses_of(service) {
            let source = format!("{}.{}", u.kind.section(), u.resource);
            let names = resource_var_names(u.kind, &u.resource);
            match u.kind {
                ResourceKind::Queue => {
                    vars.push(EnvVar {
                        name: names[0].clone(),
                        source: source.clone(),
                        local: lit(&format!("{LOCAL_SQS_ENDPOINT}/000000000000/{}", u.resource)),
                        aws: Some(AwsValue::Ref { resource: source.clone() }),
                        secret: false,
                    });
                    iam.push(IamGrant { role: Role::Task, resource: source, actions: SQS_ACTIONS.to_vec() });
                }
                ResourceKind::Dynamodb => {
                    vars.push(EnvVar {
                        name: names[0].clone(),
                        source: source.clone(),
                        local: lit(&u.resource),
                        aws: Some(AwsValue::Ref { resource: source.clone() }),
                        secret: false,
                    });
                    iam.push(IamGrant { role: Role::Task, resource: source, actions: DYNAMODB_ACTIONS.to_vec() });
                }
                ResourceKind::Postgres => {
                    debug_assert!(matches!(self.databases.get(&u.resource), Some(Database::Postgres { .. })));
                    let db = db_name(&u.resource);
                    let port = POSTGRES_PORT.to_string();
                    let parts: [(&str, LocalValue, AwsValue, bool); 5] = [
                        (
                            "HOST",
                            lit(LOCAL_POSTGRES_HOST),
                            AwsValue::GetAtt { resource: source.clone(), attribute: "Endpoint.Address".to_owned() },
                            false,
                        ),
                        (
                            "PORT",
                            lit(&port),
                            AwsValue::GetAtt { resource: source.clone(), attribute: "Endpoint.Port".to_owned() },
                            false,
                        ),
                        ("NAME", lit(&db), AwsValue::Literal { value: db.clone() }, false),
                        ("USER", lit(POSTGRES_USER), AwsValue::Literal { value: POSTGRES_USER.to_owned() }, false),
                        (
                            "PASSWORD",
                            lit(POSTGRES_USER),
                            AwsValue::ManagedSecret { resource: source.clone(), json_key: "password".to_owned() },
                            true,
                        ),
                    ];
                    for (part, local, aws, secret) in parts {
                        vars.push(EnvVar {
                            name: format!("{}_{part}", var_prefix(&u.resource)),
                            source: source.clone(),
                            local,
                            aws: Some(aws),
                            secret,
                        });
                    }
                    iam.push(IamGrant {
                        role: Role::Execution,
                        resource: source,
                        actions: vec!["secretsmanager:GetSecretValue"],
                    });
                    network.push(u.resource.clone());
                }
            }
        }

        for key in &svc.secrets {
            vars.push(EnvVar {
                name: key.clone(),
                source: format!("secrets.{key}"),
                local: LocalValue::DotEnv { key: key.clone() },
                aws: Some(AwsValue::Ssm { parameter: format!("/shipfile/{}/{env_name}/{key}", self.app.name) }),
                secret: true,
            });
        }
        if !svc.secrets.is_empty() {
            iam.push(IamGrant { role: Role::Execution, resource: "secrets".to_owned(), actions: vec!["ssm:GetParameters"] });
        }

        for (key, value) in &svc.env {
            vars.push(EnvVar {
                name: key.clone(),
                source: "env".to_owned(),
                local: lit(value),
                aws: Some(AwsValue::Literal { value: value.clone() }),
                secret: false,
            });
        }

        // Platform variables.
        let region = self.app.region.as_deref().unwrap_or(LOCAL_DEFAULT_REGION);
        let platform = |name: &str, local: LocalValue, aws: Option<AwsValue>| EnvVar {
            name: name.to_owned(),
            source: "platform".to_owned(),
            local,
            aws,
            secret: false,
        };
        vars.push(platform("AWS_REGION", lit(region), Some(AwsValue::Region)));
        vars.push(platform("AWS_ACCESS_KEY_ID", lit("local"), None));
        vars.push(platform("AWS_SECRET_ACCESS_KEY", lit("local"), None));
        let uses = |k: ResourceKind| self.uses_of(service).any(|u| u.kind == k);
        if uses(ResourceKind::Queue) {
            vars.push(platform("AWS_ENDPOINT_URL_SQS", lit(LOCAL_SQS_ENDPOINT), None));
        }
        if uses(ResourceKind::Dynamodb) {
            vars.push(platform("AWS_ENDPOINT_URL_DYNAMODB", lit(LOCAL_DYNAMODB_ENDPOINT), None));
        }

        vars.sort_by(|a, b| a.name.cmp(&b.name));
        iam.sort();
        network.sort();
        Some(EnvContract { service: service.to_owned(), vars, iam, network })
    }
}
