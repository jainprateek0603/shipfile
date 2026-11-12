//! The environment-variable contract (`docs/PLAN.md` §5.2, ADR-0006).
//!
//! [`Model::contract`] is the only place that turns `uses` edges into environment variables,
//! IAM grants and network access. The Compose generator, the CloudFormation generator and the
//! bindings all read from it, so local and AWS can't drift apart.

use serde::Serialize;

use crate::model::{Database, Model, ResourceKind};
use crate::name::Name;

/// Host names and ports of the local emulators in the generated Compose file.
pub mod local {
    pub const POSTGRES_HOST: &str = "postgres";
    pub const POSTGRES_PORT: u16 = 5432;
    pub const POSTGRES_USER: &str = "shipfile";
    pub const POSTGRES_PASSWORD: &str = "shipfile";
    pub const ELASTICMQ_ENDPOINT: &str = "http://elasticmq:9324";
    pub const DYNAMODB_ENDPOINT: &str = "http://dynamodb:8000";
    /// ElasticMQ's fixed account id in queue URLs.
    pub const ACCOUNT_ID: &str = "000000000000";
    /// Region used locally when the spec doesn't set one.
    pub const DEFAULT_REGION: &str = "us-east-1";
    /// Dummy credentials the emulators accept.
    pub const ACCESS_KEY_ID: &str = "local";
    pub const SECRET_ACCESS_KEY: &str = "local";
}

pub const SQS_ACTIONS: [&str; 6] = [
    "sqs:ChangeMessageVisibility",
    "sqs:DeleteMessage",
    "sqs:GetQueueAttributes",
    "sqs:GetQueueUrl",
    "sqs:ReceiveMessage",
    "sqs:SendMessage",
];

pub const DYNAMODB_ACTIONS: [&str; 10] = [
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

/// Everything one service is given: env vars, IAM grants and network access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnvContract {
    pub service: Name,
    /// Sorted by name.
    pub vars: Vec<EnvVar>,
    /// Sorted by role, then target.
    pub grants: Vec<Grant>,
    /// Postgres databases whose security group must accept this service.
    pub network: Vec<Name>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnvVar {
    pub name: String,
    pub local: LocalValue,
    pub aws: AwsValue,
    /// Secrets must never be printed or baked into images.
    pub secret: bool,
}

/// How a variable is set in the local Compose stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum LocalValue {
    Literal {
        value: String,
    },
    /// Read from the git-ignored `.env.local` file.
    DotEnv {
        key: String,
    },
}

/// How a variable is set on AWS. Generators turn these into `Ref`, `GetAtt` or ECS `secrets`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum AwsValue {
    Literal {
        value: String,
    },
    /// Only set locally (emulator endpoints and dummy credentials).
    Unset,
    /// The `AWS::Region` pseudo parameter.
    Region,
    /// The SQS queue URL (`Ref` of the queue).
    QueueUrl {
        queue: Name,
    },
    /// The RDS endpoint address (`GetAtt Endpoint.Address`).
    DbAddress {
        database: Name,
    },
    /// The RDS endpoint port (`GetAtt Endpoint.Port`).
    DbPort {
        database: Name,
    },
    /// A field of the RDS-managed master user secret, through ECS `secrets`.
    DbSecret {
        database: Name,
        field: String,
    },
    /// The DynamoDB table name (`Ref` of the table).
    TableName {
        table: Name,
    },
    /// An SSM `SecureString` parameter at `/<app>/<env>/<key>`, through ECS `secrets`.
    SsmParameter {
        key: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The role the application code runs as.
    Task,
    /// The role ECS uses to pull images, write logs and fetch secrets.
    Execution,
}

/// IAM permissions on one spec resource, written like the plan's source paths
/// (`queues.orders-queue`, `databases.main-db.secret`, `secrets.STRIPE_KEY`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Grant {
    pub role: Role,
    pub target: String,
    pub actions: Vec<String>,
}

/// The SSM parameter name for a secret.
pub fn ssm_parameter_name(app: &Name, env: &Name, key: &str) -> String {
    format!("/{app}/{env}/{key}")
}

/// The names `contract` generates for a resource, used to detect clashes with user variables.
pub fn resource_var_names(name: &Name, kind: ResourceKind) -> Vec<String> {
    let prefix = name.env_prefix();
    match kind {
        ResourceKind::Queue => vec![format!("{prefix}_URL")],
        ResourceKind::Postgres => {
            ["HOST", "PORT", "NAME", "USER", "PASSWORD"].iter().map(|s| format!("{prefix}_{s}")).collect()
        }
        ResourceKind::Dynamodb => vec![format!("{prefix}_TABLE")],
    }
}

/// Variables every service gets, whatever it uses.
pub const BASE_VARS: [&str; 5] =
    ["AWS_REGION", "AWS_ENDPOINT_URL_SQS", "AWS_ENDPOINT_URL_DYNAMODB", "AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY"];

fn literal(value: impl Into<String>) -> LocalValue {
    LocalValue::Literal { value: value.into() }
}

impl Model {
    /// The env-var contract for `service`. Returns `None` for an unknown service.
    #[expect(clippy::too_many_lines, reason = "one flat list of every variable reads best")]
    pub fn contract(&self, service: &Name) -> Option<EnvContract> {
        let svc = self.services.get(service)?;
        let mut vars = Vec::new();
        let mut grants = Vec::new();
        let mut network = Vec::new();
        let var = |name: String, local: LocalValue, aws: AwsValue, secret: bool| EnvVar { name, local, aws, secret };

        let region = self.app.region.clone().unwrap_or_else(|| local::DEFAULT_REGION.to_owned());
        vars.push(var("AWS_REGION".into(), literal(region), AwsValue::Region, false));

        let kinds: Vec<ResourceKind> = self.uses_of(service).map(|u| u.kind).collect();
        let uses_sqs = kinds.contains(&ResourceKind::Queue);
        let uses_dynamodb = kinds.contains(&ResourceKind::Dynamodb);
        if uses_sqs {
            vars.push(var("AWS_ENDPOINT_URL_SQS".into(), literal(local::ELASTICMQ_ENDPOINT), AwsValue::Unset, false));
        }
        if uses_dynamodb {
            vars.push(var(
                "AWS_ENDPOINT_URL_DYNAMODB".into(),
                literal(local::DYNAMODB_ENDPOINT),
                AwsValue::Unset,
                false,
            ));
        }
        if uses_sqs || uses_dynamodb {
            vars.push(var("AWS_ACCESS_KEY_ID".into(), literal(local::ACCESS_KEY_ID), AwsValue::Unset, false));
            vars.push(var("AWS_SECRET_ACCESS_KEY".into(), literal(local::SECRET_ACCESS_KEY), AwsValue::Unset, false));
        }

        for edge in self.uses_of(service) {
            let name = &edge.resource;
            let names = resource_var_names(name, edge.kind);
            match (edge.kind, self.databases.get(name)) {
                (ResourceKind::Queue, _) => {
                    let url = format!("{}/{}/{name}", local::ELASTICMQ_ENDPOINT, local::ACCOUNT_ID);
                    vars.push(var(names[0].clone(), literal(url), AwsValue::QueueUrl { queue: name.clone() }, false));
                    grants.push(Grant {
                        role: Role::Task,
                        target: format!("queues.{name}"),
                        actions: SQS_ACTIONS.map(str::to_owned).to_vec(),
                    });
                }
                (ResourceKind::Postgres, Some(Database::Postgres { .. })) => {
                    let db = || name.clone();
                    let [host, port, db_name, user, password] = <[String; 5]>::try_from(names).unwrap_or_default();
                    vars.push(var(host, literal(local::POSTGRES_HOST), AwsValue::DbAddress { database: db() }, false));
                    vars.push(var(
                        port,
                        literal(local::POSTGRES_PORT.to_string()),
                        AwsValue::DbPort { database: db() },
                        false,
                    ));
                    vars.push(var(db_name, literal(name.snake()), AwsValue::Literal { value: name.snake() }, false));
                    vars.push(var(
                        user,
                        literal(local::POSTGRES_USER),
                        AwsValue::DbSecret { database: db(), field: "username".into() },
                        false,
                    ));
                    vars.push(var(
                        password,
                        literal(local::POSTGRES_PASSWORD),
                        AwsValue::DbSecret { database: db(), field: "password".into() },
                        true,
                    ));
                    grants.push(Grant {
                        role: Role::Execution,
                        target: format!("databases.{name}.secret"),
                        actions: vec!["secretsmanager:GetSecretValue".into()],
                    });
                    network.push(name.clone());
                }
                (ResourceKind::Dynamodb, _) => {
                    vars.push(var(
                        names[0].clone(),
                        literal(name.as_str()),
                        AwsValue::TableName { table: name.clone() },
                        false,
                    ));
                    grants.push(Grant {
                        role: Role::Task,
                        target: format!("databases.{name}"),
                        actions: DYNAMODB_ACTIONS.map(str::to_owned).to_vec(),
                    });
                }
                (ResourceKind::Postgres, _) => {}
            }
        }

        for (key, value) in &svc.env {
            vars.push(var(key.clone(), literal(value.clone()), AwsValue::Literal { value: value.clone() }, false));
        }
        for key in &svc.secrets {
            vars.push(var(
                key.clone(),
                LocalValue::DotEnv { key: key.clone() },
                AwsValue::SsmParameter { key: key.clone() },
                true,
            ));
            grants.push(Grant {
                role: Role::Execution,
                target: format!("secrets.{key}"),
                actions: vec!["ssm:GetParameters".into()],
            });
        }

        vars.sort_by(|a, b| a.name.cmp(&b.name));
        grants.sort();
        network.sort();
        Some(EnvContract { service: service.clone(), vars, grants, network })
    }
}
