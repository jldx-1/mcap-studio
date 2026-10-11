use mcap_workbench::{accounts::Principal, Store};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs::File, path::Path, sync::Arc};

const PASSWORD: &str = "a-test-password-123";
struct Lab {
    _dir: tempfile::TempDir,
    store: Arc<Store>,
    admin: Principal,
    alice: Principal,
    bob: Principal,
    project: String,
    recording: String,
    folder: std::path::PathBuf,
}
fn fixture(path: &Path) {
    let mut writer = mcap::Writer::new(File::create(path).unwrap()).unwrap();
    let channel = writer
        .add_channel(0, "/camera", "json", &BTreeMap::new())
        .unwrap();
    let sensor = writer
        .add_channel(0, "/sensor", "json", &BTreeMap::new())
        .unwrap();
    for n in 0..3 {
        for (id, data) in [
            (
                channel,
                json!({"width":2,"height":2,"step":6,"encoding":"rgb8","data":[255,0,0,255,0,0,255,0,0,255,0,0]}),
            ),
            (sensor, json!({"position":n})),
        ] {
            writer
                .write_to_known_channel(
                    &mcap::records::MessageHeader {
                        channel_id: id,
                        sequence: n,
                        log_time: 1_000_000_000 + n as u64 * 100_000_000,
                        publish_time: 1_000_000_000 + n as u64 * 100_000_000,
                    },
                    &serde_json::to_vec(&data).unwrap(),
                )
                .unwrap();
        }
    }
    writer.finish().unwrap();
}
fn user(store: &Store, admin: &Principal, name: &str, group: &str) -> Principal {
    let groups = store.accounts.groups().unwrap();
    let group = groups
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["name"] == group)
        .unwrap();
    store.accounts.save_user(admin,None,&json!({"username":name,"name":name,"password":PASSWORD,"groups":[group["id"]],"permissions":[]})).unwrap();
    store
        .accounts
        .login(&json!({"username":name,"password":PASSWORD}))
        .unwrap()
        .0
}
fn lab() -> Lab {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::new(dir.path()).unwrap());
    let admin = store
        .accounts
        .setup(&json!({"username":"admin","name":"Administrator","password":PASSWORD}))
        .unwrap()
        .0;
    let alice = user(&store, &admin, "alice", "Annotators");
    let bob = user(&store, &admin, "bob", "Reviewers");
    let project = store.create_project(&json!({"name":"Robot"})).unwrap();
    let id = project["uuid"].as_str().unwrap().to_owned();
    let folder = Path::new(project["path"].as_str().unwrap()).to_owned();
    store
        .accounts
        .save_members(
            &admin,
            &folder,
            &json!({"members":[{"user_id":alice.id},{"user_id":bob.id}]}),
        )
        .unwrap();
    fixture(&dir.path().join("input.mcap"));
    let recording = store
        .import_recording(&id, "input.mcap", &dir.path().join("input.mcap"))
        .unwrap();
    let recording = recording["uuid"].as_str().unwrap().to_owned();
    store
        .add_named(&id, "labels", &json!({"name":"robot"}))
        .unwrap();
    let mut state = store.recording(&id, &recording).unwrap()["annotations"].clone();
    state["techniques"]["1"] = json!("classification");
    state["annotations"] = json!({"1":{"0":{"reviewed":false,"boxes":[],"labels":[0]},"1":{"reviewed":true,"boxes":[],"labels":[]},"2":{"reviewed":false,"boxes":[],"labels":[0]}}});
    store
        .save_annotations_as(&id, &recording, &state, &alice)
        .unwrap();
    Lab {
        _dir: dir,
        store,
        admin,
        alice,
        bob,
        project: id,
        recording,
        folder,
    }
}
fn task(lab: &Lab) -> Value {
    lab.store.create_review_task(&lab.project,&lab.alice,&json!({"title":"Video 001","recording_id":lab.recording,"scopes":[{"channel_id":"1","start_frame":0,"end_frame":2}]})).unwrap()
}
fn action(
    lab: &Lab,
    task: &Value,
    actor: &Principal,
    action: &str,
    extra: Value,
) -> anyhow::Result<Value> {
    let mut request = json!({"action":action,"expected_version":task["version"]});
    request
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    lab.store
        .review_task_action(&lab.project, task["id"].as_str().unwrap(), actor, &request)
}

#[test]
fn submitted_snapshots_and_comments_are_permanent_across_working_edits() {
    let lab = lab();
    let created = task(&lab);
    let first = action(&lab, &created, &lab.alice, "submit", json!({})).unwrap();
    assert_eq!(first["current_revision"], 1);
    let first = action(&lab, &first, &lab.bob, "claim", json!({})).unwrap();
    let feedback=action(&lab,&first,&lab.bob,"request_changes",json!({"comments":[{"message":"Fix frame one","scope":{"channel_id":"1","start_frame":0,"end_frame":0}},{"message":"Check later frames","scope":{"channel_id":"1","start_frame":1,"end_frame":2}}]})).unwrap();
    let old_path = lab.folder.join(format!(
        "reviews/{}/{}/revisions/000000000001.json",
        lab.recording,
        created["id"].as_str().unwrap()
    ));
    let old_bytes = std::fs::read(&old_path).unwrap();
    let mut working =
        lab.store.recording(&lab.project, &lab.recording).unwrap()["annotations"].clone();
    working["annotations"]["1"]["0"]["labels"] = json!([]);
    lab.store
        .save_annotations_as(&lab.project, &lab.recording, &working, &lab.alice)
        .unwrap();
    let old = lab
        .store
        .review_revision_recording(&lab.project, created["id"].as_str().unwrap(), 1, &lab.bob)
        .unwrap();
    assert_eq!(
        old["annotations"]["annotations"]["1"]["0"]["labels"],
        json!([0])
    );
    let fresh = lab
        .store
        .review_tasks(&lab.project, &lab.alice, false)
        .unwrap()[0]
        .clone();
    let second = action(&lab, &fresh, &lab.alice, "submit", json!({})).unwrap();
    assert_eq!(second["current_revision"], 2);
    assert_eq!(std::fs::read(&old_path).unwrap(), old_bytes);
    let revisions = lab
        .store
        .review_revisions(&lab.project, created["id"].as_str().unwrap(), &lab.bob)
        .unwrap();
    assert_eq!(revisions[1]["comments"].as_array().unwrap().len(), 2);
    assert_eq!(revisions[0]["comments"].as_array().unwrap().len(), 0);
    let diff = lab
        .store
        .review_revision_diff(
            &lab.project,
            created["id"].as_str().unwrap(),
            2,
            1,
            &lab.bob,
            (0, 100),
        )
        .unwrap();
    assert_eq!(diff["total"], 1);
    assert_eq!(diff["changes"][0]["frame"], 0);
    assert!(lab.store.review_task_action(&lab.project,created["id"].as_str().unwrap(),&lab.bob,&json!({"action":"comment","revision_number":1,"expected_revision_version":revisions[1]["version"],"message":"Cannot change old comments"})).is_err());
    let _ = feedback;
}
#[test]
fn dataset_versions_keep_their_approved_inputs_when_new_revisions_are_approved() {
    let lab = lab();
    let created = task(&lab);
    let first = action(&lab, &created, &lab.alice, "submit", json!({})).unwrap();
    let first = action(&lab, &first, &lab.bob, "claim", json!({})).unwrap();
    action(&lab, &first, &lab.bob, "approve", json!({})).unwrap();
    let ready = lab.store.approved_data(&lab.project).unwrap();
    assert_eq!(ready["sources"].as_array().unwrap().len(), 1);
    let made=lab.store.create_dataset_version(&lab.project,None,&lab.admin,&json!({"name":"Robot training","modalities":["rgb","structured"],"sources":[{"task_id":created["id"],"revision":1}]})).unwrap();
    let dataset = made["dataset"]["uuid"].as_str().unwrap();
    assert_eq!(made["version"]["sample_count"], 3);
    let path = lab.folder.join(format!(
        "datasets/{dataset}/versions/000000000001/version.json"
    ));
    let frozen = std::fs::read(&path).unwrap();
    let built = lab
        .store
        .build_dataset_version(&lab.project, dataset, 1, &json!({"format":"csv"}))
        .unwrap();
    assert_eq!(built["sample_count"], 3);
    assert_eq!(
        lab.store
            .build_dataset_version(&lab.project, dataset, 1, &json!({"format":"csv"}))
            .unwrap()["updated_samples"],
        0
    );
    let mut working =
        lab.store.recording(&lab.project, &lab.recording).unwrap()["annotations"].clone();
    working["annotations"]["1"]["0"]["labels"] = json!([]);
    lab.store
        .save_annotations_as(&lab.project, &lab.recording, &working, &lab.alice)
        .unwrap();
    let fresh = lab
        .store
        .review_tasks(&lab.project, &lab.alice, false)
        .unwrap()[0]
        .clone();
    let second = action(&lab, &fresh, &lab.alice, "submit", json!({})).unwrap();
    let second = action(&lab, &second, &lab.bob, "claim", json!({})).unwrap();
    action(&lab, &second, &lab.bob, "approve", json!({})).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), frozen);
    assert_eq!(
        lab.store.dataset_version(&lab.project, dataset, 1).unwrap()["source_revisions"][0]
            ["revision"],
        1
    );
    let next=lab.store.create_dataset_version(&lab.project,Some(dataset),&lab.admin,&json!({"modalities":["rgb","structured"],"sources":[{"task_id":created["id"],"revision":2}]})).unwrap();
    assert_eq!(next["version"]["number"], 2);
    assert_eq!(next["version"]["sample_count"], 2);
    assert_eq!(
        lab.store.approved_data(&lab.project).unwrap()["sources"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    lab.store
        .accounts
        .delete_user(&lab.admin, &lab.bob.id)
        .unwrap();
    assert_eq!(
        lab.store.approved_data(&lab.project).unwrap()["sources"][0]["reviewer"],
        "bob"
    );
    assert!(lab
        .store
        .accounts
        .delete_user(&lab.admin, &lab.admin.id)
        .is_err());
}
