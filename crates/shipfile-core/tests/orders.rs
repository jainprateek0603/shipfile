//! Golden tests for `examples/orders`: the model and every service's env contract.

use std::path::PathBuf;

use shipfile_core::{DirProject, Loaded, Name, load};

fn orders(env: Option<&str>) -> Loaded {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/orders");
    let source = std::fs::read_to_string(dir.join("shipfile.yaml")).expect("example spec");
    match load(&source, env, &DirProject::new(dir)) {
        Ok(loaded) => loaded,
        Err(failed) => panic!("examples/orders must pass check: {:#?}", failed.diagnostics),
    }
}

#[test]
fn contract_for_every_service() {
    let model = orders(None).model;
    for service in model.services.keys() {
        insta::assert_json_snapshot!(format!("contract-{service}"), model.contract(service).expect("known service"));
    }
}

#[test]
fn prod_overlay_applies() {
    let model = orders(Some("prod")).model;
    let api = &model.services[&Name::new("api").unwrap()];
    assert_eq!(api.scale.replicas, 2);
    insta::assert_json_snapshot!("model-prod", model);
}

#[test]
fn unknown_service_has_no_contract() {
    assert!(orders(None).model.contract(&Name::new("nope").unwrap()).is_none());
}
