//! Applying an environment's overrides to the base spec.
//!
//! Rules: scalars and lists replace, `scale` and `dlq` merge field by field, `env` merges key by
//! key with the override winning. Overrides for resources that don't exist are reported by
//! [`crate::validate`] and ignored here.

use crate::spec::{
    DatabasePatch, QueuePatch, RawAws, RawDatabase, RawDlq, RawEnvironment, RawQueue, RawScale, RawService, RawSpec,
    ServicePatch,
};

/// The spec with `env`'s overrides applied. The result has no `environments` left to apply.
pub fn apply(spec: &RawSpec, env: &RawEnvironment) -> RawSpec {
    let mut out = spec.clone();
    if let Some(aws) = &env.aws {
        out.aws = Some(merge_aws(spec.aws.as_ref(), aws));
    }
    for (name, patch) in &env.services {
        if let Some(service) = out.services.get_mut(name) {
            *service = merge_service(service, patch);
        }
    }
    for (name, patch) in &env.queues {
        if let Some(queue) = out.queues.get_mut(name) {
            *queue = Some(merge_queue(&queue.unwrap_or_default(), patch));
        }
    }
    for (name, patch) in &env.databases {
        if let Some(database) = out.databases.get_mut(name) {
            *database = merge_database(database, patch);
        }
    }
    out
}

fn merge_aws(base: Option<&RawAws>, patch: &RawAws) -> RawAws {
    RawAws { region: patch.region.clone().or_else(|| base.and_then(|b| b.region.clone())) }
}

pub fn merge_service(base: &RawService, patch: &ServicePatch) -> RawService {
    let mut env = base.env.clone();
    env.extend(patch.env.iter().map(|(k, v)| (k.clone(), v.clone())));
    RawService {
        build: base.build.clone(),
        runtime: base.runtime.clone(),
        port: patch.port.or(base.port),
        public: patch.public.or(base.public),
        health: patch.health.clone().or_else(|| base.health.clone()),
        scale: merge_scale(base.scale, patch.scale),
        env,
        secrets: patch.secrets.clone().unwrap_or_else(|| base.secrets.clone()),
        uses: base.uses.clone(),
        command: patch.command.clone().or_else(|| base.command.clone()),
    }
}

pub fn merge_scale(base: Option<RawScale>, patch: Option<RawScale>) -> Option<RawScale> {
    match (base, patch) {
        (base, None) => base,
        (None, patch) => patch,
        (Some(b), Some(p)) => {
            Some(RawScale { cpu: p.cpu.or(b.cpu), memory: p.memory.or(b.memory), replicas: p.replicas.or(b.replicas) })
        }
    }
}

pub fn merge_queue(base: &RawQueue, patch: &QueuePatch) -> RawQueue {
    let dlq = match (base.dlq, patch.dlq) {
        (base, None) => base,
        (None, patch) => patch,
        (Some(b), Some(p)) => Some(RawDlq { max_receives: p.max_receives.or(b.max_receives) }),
    };
    RawQueue { dlq }
}

pub fn merge_database(base: &RawDatabase, patch: &DatabasePatch) -> RawDatabase {
    RawDatabase { size: patch.size.or(base.size), ..base.clone() }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use proptest::collection::{btree_map, vec};
    use proptest::option;
    use proptest::prelude::*;

    use super::*;
    use crate::spec::EnvValue;

    fn scale() -> impl Strategy<Value = RawScale> {
        (option::of(Just(512u32)), option::of(512u32..4096), option::of(0u32..5))
            .prop_map(|(cpu, memory, replicas)| RawScale { cpu, memory, replicas })
    }

    fn env_map() -> impl Strategy<Value = BTreeMap<String, EnvValue>> {
        btree_map("[A-D]", "[a-c]{1,2}".prop_map(EnvValue::String), 0..4)
    }

    fn service() -> impl Strategy<Value = RawService> {
        (
            option::of(1u32..9000),
            option::of(any::<bool>()),
            option::of("/[a-z]{1,4}"),
            option::of(scale()),
            env_map(),
            vec("[A-C]", 0..3),
            option::of(vec("[a-z]{1,3}", 1..3)),
        )
            .prop_map(|(port, public, health, scale, env, secrets, command)| RawService {
                build: "./svc".into(),
                runtime: Some("node22".into()),
                port,
                public,
                health,
                scale,
                env,
                secrets,
                uses: vec!["q".into()],
                command,
            })
    }

    fn patch() -> impl Strategy<Value = ServicePatch> {
        (
            option::of(1u32..9000),
            option::of(any::<bool>()),
            option::of("/[a-z]{1,4}"),
            option::of(scale()),
            env_map(),
            option::of(vec("[A-C]", 0..3)),
            option::of(vec("[a-z]{1,3}", 1..3)),
        )
            .prop_map(|(port, public, health, scale, env, secrets, command)| ServicePatch {
                port,
                public,
                health,
                scale,
                env,
                secrets,
                command,
            })
    }

    proptest! {
        #[test]
        fn empty_patch_is_identity(base in service()) {
            prop_assert_eq!(merge_service(&base, &ServicePatch::default()), base);
        }

        #[test]
        fn merge_is_idempotent(base in service(), p in patch()) {
            let once = merge_service(&base, &p);
            prop_assert_eq!(merge_service(&once, &p), once);
        }

        #[test]
        fn set_fields_win_and_unset_fields_keep_the_base(base in service(), p in patch()) {
            let merged = merge_service(&base, &p);
            prop_assert_eq!(merged.port, p.port.or(base.port));
            prop_assert_eq!(merged.public, p.public.or(base.public));
            prop_assert_eq!(merged.health.as_ref(), p.health.as_ref().or(base.health.as_ref()));
            prop_assert_eq!(&merged.secrets, p.secrets.as_ref().unwrap_or(&base.secrets));
            prop_assert_eq!(merged.command.as_ref(), p.command.as_ref().or(base.command.as_ref()));
            // Fields an environment can't change never change.
            prop_assert_eq!(&merged.build, &base.build);
            prop_assert_eq!(&merged.runtime, &base.runtime);
            prop_assert_eq!(&merged.uses, &base.uses);
        }

        #[test]
        fn env_merges_key_by_key(base in service(), p in patch()) {
            let merged = merge_service(&base, &p);
            for (key, value) in &merged.env {
                prop_assert_eq!(Some(value), p.env.get(key).or_else(|| base.env.get(key)));
            }
            prop_assert!(base.env.keys().chain(p.env.keys()).all(|k| merged.env.contains_key(k)));
        }

        #[test]
        fn scale_merges_field_by_field(base in option::of(scale()), p in option::of(scale())) {
            let merged = merge_scale(base, p).unwrap_or_default();
            let (b, q) = (base.unwrap_or_default(), p.unwrap_or_default());
            prop_assert_eq!(merged.cpu, q.cpu.or(b.cpu));
            prop_assert_eq!(merged.memory, q.memory.or(b.memory));
            prop_assert_eq!(merged.replicas, q.replicas.or(b.replicas));
        }
    }

    #[test]
    fn apply_ignores_unknown_resources_and_merges_aws() {
        let spec: RawSpec = serde_json::from_value(serde_json::json!({
            "version": 1, "name": "x", "aws": {"region": "us-east-1"},
            "services": {"api": {"build": ".", "scale": {"replicas": 1, "cpu": 512}}},
            "databases": {"db": {"engine": "postgres"}}
        }))
        .unwrap();
        let env: RawEnvironment = serde_json::from_value(serde_json::json!({
            "aws": {"region": "ap-south-1"},
            "services": {"api": {"scale": {"replicas": 2}}, "ghost": {"port": 1}},
            "databases": {"db": {"size": "medium"}}
        }))
        .unwrap();
        let out = apply(&spec, &env);
        assert_eq!(out.aws.unwrap().region.as_deref(), Some("ap-south-1"));
        assert_eq!(out.services["api"].scale, Some(RawScale { cpu: Some(512), memory: None, replicas: Some(2) }));
        assert_eq!(out.databases["db"].size, Some(crate::spec::Size::Medium));
        assert!(!out.services.contains_key("ghost"));
    }
}
