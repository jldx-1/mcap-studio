use mcap_workbench::{accounts::Principal, Store};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};

fn fixture(path: &Path) {
    let mut writer = mcap::Writer::new(fs::File::create(path).unwrap()).unwrap();
    let camera = writer
        .add_channel(0, "/camera", "json", &BTreeMap::new())
        .unwrap();
    let motor = writer
        .add_channel(0, "/motor", "json", &BTreeMap::new())
        .unwrap();
    let debug = writer
        .add_channel(0, "/debug", "json", &BTreeMap::new())
        .unwrap();
    for sequence in 0..3 {
        for (channel_id, value) in [
            (
                camera,
                json!({"width":2,"height":1,"step":6,"encoding":"rgb8","data":[255,0,0,0,255,0]}),
            ),
            (motor, json!({"position":sequence})),
            (debug, json!({"debug":true})),
        ] {
            writer
                .write_to_known_channel(
                    &mcap::records::MessageHeader {
                        channel_id,
                        sequence,
                        log_time: 1_000_000_000 + sequence as u64 * 1_000_000,
                        publish_time: 1_000_000_000 + sequence as u64 * 1_000_000,
                    },
                    &serde_json::to_vec(&value).unwrap(),
                )
                .unwrap();
        }
    }
    writer.finish().unwrap();
}
fn lab() -> (tempfile::TempDir, Arc<Store>, Principal, Value, Value) {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::new(dir.path()).unwrap());
    let actor = store
        .accounts
        .setup(&json!({"username":"admin","name":"Admin","password":"test-password-123"}))
        .unwrap()
        .0;
    let project = store.create_project(&json!({"name":"Workflow"})).unwrap();
    fs::create_dir_all(dir.path().join("imports")).unwrap();
    fixture(&dir.path().join("imports/input.mcap"));
    let recording = store
        .import_server_recording(project["uuid"].as_str().unwrap(), "input.mcap", &actor)
        .unwrap();
    (dir, store, actor, project, recording)
}

#[test]
fn recipes_and_pending_dataset_drafts_survive_reload_and_reject_stale_updates() {
    let (dir, store, actor, project, recording) = lab();
    let id = project["uuid"].as_str().unwrap();
    let draft = store.save_workflow_item(id,"drafts",&json!({"name":"Pending training","recordings":[{"project_uuid":id,"recording_id":recording["uuid"]}],"selected_sources":[],"target_dataset":null}),&actor).unwrap();
    assert!(store
        .delete_recording(id, recording["uuid"].as_str().unwrap())
        .is_err());
    let topics = store
        .topic_translations(id, recording["uuid"].as_str().unwrap())
        .unwrap();
    let rules:Vec<_> = topics["topics"].as_array().unwrap().iter().map(|t| json!({"topic":t["topic"],"source_message":t["source_message"],"source_schema_sha256":t["source_schema_sha256"],"keep":t["topic"]!="/debug","required":true,"script":""})).collect();
    let recipe = store
        .save_workflow_item(
            id,
            "recipes",
            &json!({"name":"Robot recipe","rules":rules}),
            &actor,
        )
        .unwrap();
    let old_bytes = fs::read(Path::new(project["path"].as_str().unwrap()).join(format!(
        "workflow/recipe-history/{}/000000000001.json",
        recipe["uuid"].as_str().unwrap()
    )))
    .unwrap();
    let mut changed = recipe.clone();
    changed["name"] = json!("Updated recipe");
    let updated = store
        .save_workflow_item(id, "recipes", &changed, &actor)
        .unwrap();
    assert_eq!(updated["revision"], 2);
    assert_ne!(updated["sha256"], recipe["sha256"]);
    assert!(store
        .save_workflow_item(id, "recipes", &changed, &actor)
        .is_err());
    assert_eq!(
        fs::read(Path::new(project["path"].as_str().unwrap()).join(format!(
            "workflow/recipe-history/{}/000000000001.json",
            recipe["uuid"].as_str().unwrap()
        )))
        .unwrap(),
        old_bytes
    );
    let restored = Store::new(dir.path()).unwrap();
    assert_eq!(restored.workflow_items(id, "drafts").unwrap()[0], draft);
    assert_eq!(restored.workflow_items(id, "recipes").unwrap()[0], updated);
    let mut update = draft.clone();
    update["name"] = json!("Saved again");
    restored
        .save_workflow_item(id, "drafts", &update, &actor)
        .unwrap();
    assert!(restored
        .save_workflow_item(id, "drafts", &update, &actor)
        .is_err());
    restored
        .delete_workflow_item(id, "drafts", draft["uuid"].as_str().unwrap(), &actor)
        .unwrap();
    assert!(restored
        .workflow_items(id, "drafts")
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn durable_preparation_filters_topics_preserves_source_and_reuses_identical_outputs() {
    let (dir, store, actor, project, recording) = lab();
    let id = project["uuid"].as_str().unwrap();
    let source = recording["uuid"].as_str().unwrap();
    let source_path =
        Path::new(project["path"].as_str().unwrap()).join(recording["path"].as_str().unwrap());
    let original = fs::read(&source_path).unwrap();
    let label_project = store
        .add_named(id, "labels", &json!({"name":"robot"}))
        .unwrap();
    let list_project = store
        .add_named(
            id,
            "label_lists",
            &json!({"name":"Objects","label_ids":[label_project["labels"][0]["uuid"]]}),
        )
        .unwrap();
    let request = json!({"recording_uuid":source,"selected_channels":["1","2"],"translations":{"2":"millimeters <- source.position * 1000"},"techniques":{"1":"classification"},"topic_settings":{"1":{"technique":"classification","label_list_id":list_project["label_lists"][0]["uuid"]}},"sync":{"enabled":false}});
    let job = store.queue_preparation(id, &request, &actor).unwrap();
    assert_eq!(job["status"], "queued");
    assert!(store.delete_recording(id, source).is_err());
    let restored = Store::new(dir.path()).unwrap();
    assert!(restored.run_next_preparation(true).unwrap());
    let result = restored.workflow_items(id, "jobs").unwrap()[0].clone();
    assert_eq!(result["status"], "ready", "{result}");
    let output = result["output_recording"].as_str().unwrap();
    let prepared = restored.recording(id, output).unwrap();
    assert_eq!(prepared["topics"].as_array().unwrap().len(), 2);
    assert!(prepared["topics"]
        .as_array()
        .unwrap()
        .iter()
        .all(|t| t["topic"] != "/debug"));
    assert_eq!(prepared["annotations"]["techniques"]["1"], "classification");
    assert_eq!(
        prepared["annotations"]["topic_label_lists"]["1"],
        list_project["label_lists"][0]["uuid"]
    );
    assert_eq!(
        restored.record(id, output, "2", 2).unwrap()["millimeters"],
        2000
    );
    assert_eq!(restored.frame(id, output, "1", 0).unwrap().1, "image/png");
    assert_eq!(fs::read(&source_path).unwrap(), original);
    assert!(!source_path
        .parent()
        .unwrap()
        .join("frames/index.json")
        .exists());
    let repeated = restored.queue_preparation(id, &request, &actor).unwrap();
    assert!(restored.run_next_preparation(false).unwrap());
    let all = restored.workflow_items(id, "jobs").unwrap();
    let repeated = all
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["uuid"] == repeated["uuid"])
        .unwrap();
    assert_eq!(repeated["output_recording"], output);
    assert_eq!(
        restored.project(id).unwrap()["mcaps"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(!restored.run_next_preparation(false).unwrap());
}

#[test]
fn server_import_deduplicates_and_rejects_traversal_and_permissions() {
    let (_dir, store, actor, project, recording) = lab();
    let id = project["uuid"].as_str().unwrap();
    let second = store
        .import_server_recording(id, "input.mcap", &actor)
        .unwrap();
    assert_eq!(second["uuid"], recording["uuid"]);
    assert_eq!(second["duplicate"], true);
    assert!(store
        .import_server_recording(id, "../input.mcap", &actor)
        .is_err());
    assert!(store.server_recordings(id, "../", &actor).is_err());
    assert_eq!(
        store.server_recordings(id, "", &actor).unwrap()["entries"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    store.accounts.save_user(&actor,None,&json!({"username":"viewer","name":"Viewer","password":"test-password-123","groups":[],"permissions":["project.view","mcap.view"]})).unwrap();
    let viewer = store
        .accounts
        .login(&json!({"username":"viewer","password":"test-password-123"}))
        .unwrap()
        .0;
    store
        .accounts
        .save_members(
            &actor,
            Path::new(project["path"].as_str().unwrap()),
            &json!({"members":[{"user_id":viewer.id}]}),
        )
        .unwrap();
    assert!(store
        .import_server_recording(id, "input.mcap", &viewer)
        .is_err());
    assert!(store
        .save_workflow_item(
            id,
            "drafts",
            &json!({"name":"Denied","recordings":[]}),
            &viewer
        )
        .is_err());
    assert!(store.queue_preparation(id,&json!({"recording_uuid":recording["uuid"],"selected_channels":["1"],"translations":{}}),&viewer).is_err());
}

#[test]
fn interrupted_jobs_resume_and_session_groups_cover_prepared_variants() {
    let (_dir, store, actor, project, recording) = lab();
    let id = project["uuid"].as_str().unwrap();
    let job=store.queue_preparation(id,&json!({"recording_uuid":recording["uuid"],"selected_channels":["1"],"translations":{},"sync":{"enabled":false}}),&actor).unwrap();
    let mut interrupted = job.clone();
    interrupted["status"] = json!("running");
    let path = Path::new(project["path"].as_str().unwrap()).join(format!(
        "workflow/jobs/{}.json",
        job["uuid"].as_str().unwrap()
    ));
    fs::write(path, serde_json::to_vec(&interrupted).unwrap()).unwrap();
    assert!(store.run_next_preparation(true).unwrap());
    let output = store.workflow_items(id, "jobs").unwrap()[0]["output_recording"].clone();
    let grouped = store
        .set_recording_session(
            id,
            &json!({"recordings":[output],"session_id":"Capture run A"}),
            &actor,
        )
        .unwrap();
    assert!(grouped["mcaps"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["session_id"] == "Capture run A"));
}
