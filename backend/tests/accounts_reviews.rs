use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use mcap_workbench::{accounts::Principal, router, storage::fingerprint, Store};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs::File, path::Path, sync::Arc};
use tower::ServiceExt;

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
    if !["submit", "reopen", "assign_annotator"].contains(&action)
        && task["latest_revision"].is_object()
    {
        request["expected_revision_version"] = task["latest_revision"]["version"].clone();
        request["revision_number"] = task["latest_revision"]["number"].clone();
    }
    request
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    lab.store
        .review_task_action(&lab.project, task["id"].as_str().unwrap(), actor, &request)
}
async fn http(
    store: Arc<Store>,
    actor: Option<&Principal>,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("x-requested-with", "mcap-workbench");
    if let Some(actor) = actor {
        let token = store
            .accounts
            .login(&json!({"username":actor.username,"password":PASSWORD}))
            .unwrap()
            .1;
        request = request.header("cookie", format!("mcap_session={token}"));
    }
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    let response = router(store)
        .oneshot(
            request
                .body(
                    body.map(|v| Body::from(v.to_string()))
                        .unwrap_or_else(Body::empty),
                )
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn accounts_are_required_and_project_membership_is_enforced_on_every_resource() {
    let lab = lab();
    assert_eq!(
        http(lab.store.clone(), None, "GET", "/api/projects", None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        http(
            lab.store.clone(),
            Some(&lab.alice),
            "GET",
            "/api/accounts/users",
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        http(
            lab.store.clone(),
            Some(&lab.alice),
            "POST",
            "/api/projects/open",
            Some(json!({"path":"/tmp"}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let hidden = lab
        .store
        .create_project(&json!({"name":"Private"}))
        .unwrap();
    let hidden = hidden["uuid"].as_str().unwrap();
    let (_, projects) = http(
        lab.store.clone(),
        Some(&lab.alice),
        "GET",
        "/api/projects",
        None,
    )
    .await;
    assert_eq!(projects.as_array().unwrap().len(), 1);
    for path in [
        format!("/api/projects/{hidden}"),
        format!(
            "/api/projects/{hidden}/recordings/{}/frames/1/0",
            lab.recording
        ),
        format!("/api/projects/{hidden}/reviews"),
    ] {
        assert_eq!(
            http(lab.store.clone(), Some(&lab.bob), "GET", &path, None)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    let patch = json!({"sha256":lab.store.recording(&lab.project,&lab.recording).unwrap()["sha256"],"return_delta":true,"changes":[{"channel_id":"1","frame":0,"annotation":null}]});
    assert_eq!(
        http(
            lab.store.clone(),
            Some(&lab.bob),
            "PATCH",
            &format!(
                "/api/projects/{}/recordings/{}/annotations",
                lab.project, lab.recording
            ),
            Some(patch)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        http(
            lab.store.clone(),
            Some(&lab.alice),
            "GET",
            &format!("/api/projects/{}/reviews", lab.project),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}

#[test]
fn revision_review_preserves_submissions_and_requires_an_independent_reviewer() {
    let lab = lab();
    let raw = lab
        .folder
        .join(format!("mcap/{}/recording.mcap", lab.recording));
    let raw_before = fingerprint(&raw).unwrap();
    let annotations = lab
        .folder
        .join(format!("annotations/{}/metadata.json", lab.recording));
    let annotations_before = fingerprint(&annotations).unwrap();
    let task = task(&lab);
    let submitted = action(&lab, &task, &lab.alice, "submit", json!({})).unwrap();
    assert_eq!(submitted["status"], "submitted");
    assert_eq!(submitted["annotated_count"], 3);
    let mut state =
        lab.store.recording(&lab.project, &lab.recording).unwrap()["annotations"].clone();
    state["annotations"]["1"]["0"]["labels"] = json!([]);
    assert!(lab
        .store
        .save_annotations_as(&lab.project, &lab.recording, &state, &lab.alice)
        .is_ok());
    lab.store.accounts.save_members(&lab.admin,&lab.folder,&json!({"members":[{"user_id":lab.alice.id,"permissions":["review.view","review.approve"]},{"user_id":lab.bob.id}]})).unwrap();
    assert!(action(&lab, &submitted, &lab.alice, "claim", json!({}))
        .unwrap_err()
        .to_string()
        .contains("independent reviewer"));
    let reviewing = action(&lab, &submitted, &lab.bob, "claim", json!({})).unwrap();
    assert_eq!(reviewing["status"], "under_review");
    assert!(action(&lab, &submitted, &lab.bob, "claim", json!({})).is_err());
    let comment=action(&lab,&reviewing,&lab.bob,"comment",json!({"message":"Check the object edge","scope":{"channel_id":"1","start_frame":0,"end_frame":1}})).unwrap();
    let rejected=action(&lab,&comment,&lab.bob,"request_changes",json!({"message":"Fix these frames","scope":{"channel_id":"1","start_frame":0,"end_frame":1}})).unwrap();
    assert_eq!(rejected["status"], "changes_requested");
    lab.store
        .save_annotations_as(&lab.project, &lab.recording, &state, &lab.alice)
        .unwrap();
    let fresh = lab
        .store
        .review_tasks(&lab.project, &lab.alice, false)
        .unwrap()[0]
        .clone();
    let submitted = action(&lab, &fresh, &lab.alice, "submit", json!({})).unwrap();
    assert_eq!(submitted["status"], "submitted");
    assert_eq!(
        submitted["previous_revision"]["status"],
        "changes_requested"
    );
    assert_eq!(submitted["previous_revision"]["comment_count"], 2);
    let reviewing = action(&lab, &submitted, &lab.bob, "claim", json!({})).unwrap();
    let approved = action(&lab, &reviewing, &lab.bob, "approve", json!({})).unwrap();
    assert_eq!(approved["status"], "approved");
    assert!(lab
        .store
        .save_annotations_as(&lab.project, &lab.recording, &state, &lab.alice)
        .is_ok()); // no-op writes are safe
    let mut changed = state.clone();
    changed["annotations"]["1"]["0"]["labels"] = json!([0]);
    assert!(lab
        .store
        .save_annotations_as(&lab.project, &lab.recording, &changed, &lab.alice)
        .is_ok());
    assert_eq!(fingerprint(&raw).unwrap(), raw_before);
    assert_ne!(fingerprint(&annotations).unwrap(), annotations_before); // only the annotator edit touched annotation metadata
    let history = approved["history"].as_array().unwrap();
    assert!(history
        .iter()
        .any(|e| e["action"] == "task.request_changes" && e["actor_id"] == lab.bob.id));
    assert!(history
        .iter()
        .any(|e| e["action"] == "annotations.updated" && e["actor_id"] == lab.alice.id));
    let fresh = lab
        .store
        .review_tasks(&lab.project, &lab.alice, false)
        .unwrap()[0]
        .clone();
    let reopened = action(&lab, &fresh, &lab.alice, "reopen", json!({})).unwrap();
    assert_eq!(reopened["status"], "approved");
    lab.store
        .save_annotations_as(&lab.project, &lab.recording, &changed, &lab.alice)
        .unwrap();
}

#[test]
fn production_datasets_require_whole_revision_approval_and_keep_approved_inputs() {
    let lab = lab();
    let task = task(&lab);
    let submitted = action(&lab, &task, &lab.alice, "submit", json!({})).unwrap();
    let reviewing = action(&lab, &submitted, &lab.bob, "claim", json!({})).unwrap();
    assert!(action(
        &lab,
        &reviewing,
        &lab.bob,
        "approve_section",
        json!({"scope":{"channel_id":"1","start_frame":1,"end_frame":1}})
    )
    .is_err());
    assert!(lab.store.create_dataset_version(&lab.project,None,&lab.admin,&json!({"name":"Not ready","modalities":["rgb"],"sources":[{"task_id":task["id"],"revision":1}]})).is_err());
    let approved = action(&lab, &reviewing, &lab.bob, "approve", json!({})).unwrap();
    assert_eq!(
        lab.store
            .recording_review_counts(&lab.project, &lab.recording)
            .unwrap()["1"],
        3
    );
    let made=lab.store.create_dataset_version(&lab.project,None,&lab.admin,&json!({"name":"Training","modalities":["rgb"],"sources":[{"task_id":task["id"],"revision":1}]})).unwrap();
    let dataset = made["dataset"]["uuid"].as_str().unwrap();
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
    action(&lab, &approved, &lab.alice, "reopen", json!({})).unwrap();
    assert_eq!(
        lab.store
            .recording_review_counts(&lab.project, &lab.recording)
            .unwrap()["1"],
        3
    );
    assert_eq!(
        lab.store.dataset_version(&lab.project, dataset, 1).unwrap()["source_revisions"][0]
            ["revision"],
        1
    );
}

#[test]
fn project_groups_grants_and_denials_are_dynamic_and_disabled_accounts_lose_sessions() {
    let lab = lab();
    let (_, token) = lab
        .store
        .accounts
        .login(&json!({"username":"alice","password":PASSWORD}))
        .unwrap();
    lab.store.accounts.save_members(&lab.admin,&lab.folder,&json!({"members":[{"user_id":lab.alice.id,"permissions":["labels.manage"],"denied_permissions":["annotation.delete"]}]})).unwrap();
    let permissions = lab
        .store
        .accounts
        .project_permissions(&lab.alice, &lab.folder)
        .unwrap();
    assert!(permissions.contains("labels.manage"));
    assert!(!permissions.contains("annotation.delete"));
    assert!(permissions.contains("annotation.create"));
    assert!(lab
        .store
        .accounts
        .save_members(
            &lab.admin,
            &lab.folder,
            &json!({"members":[{"user_id":lab.alice.id,"permissions":["users.manage"]}]})
        )
        .is_err());
    let users = lab.store.accounts.users().unwrap();
    assert!(users
        .as_array()
        .unwrap()
        .iter()
        .all(|u| u.get("password_hash").is_none()));
    let mut alice = users
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["id"] == lab.alice.id)
        .unwrap()
        .clone();
    alice["enabled"] = json!(false);
    lab.store
        .accounts
        .save_user(&lab.admin, Some(&lab.alice.id), &alice)
        .unwrap();
    assert!(lab.store.accounts.authenticate(&token).is_err());
    assert!(lab
        .store
        .accounts
        .project_permissions(&lab.alice, &lab.folder)
        .is_err());
    let mut admin = users
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["id"] == lab.admin.id)
        .unwrap()
        .clone();
    admin["enabled"] = json!(false);
    assert!(lab
        .store
        .accounts
        .save_user(&lab.admin, Some(&lab.admin.id), &admin)
        .is_err());
}

#[tokio::test]
async fn csrf_checks_and_label_changes_cannot_bypass_approval() {
    let lab = lab();
    let (_, token) = lab
        .store
        .accounts
        .login(&json!({"username":"admin","password":PASSWORD}))
        .unwrap();
    let response = router(lab.store.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/projects")
                .header("cookie", format!("mcap_session={token}"))
                .header("content-type", "application/json")
                .body(Body::from("{\"name\":\"CSRF\"}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = router(lab.store.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/projects")
                .header("cookie", format!("mcap_session={token}"))
                .header("x-requested-with", "mcap-workbench")
                .header("origin", "https://untrusted.example")
                .header("host", "localhost:8000")
                .header("content-type", "application/json")
                .body(Body::from("{\"name\":\"CSRF\"}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let task = task(&lab);
    let submitted = action(&lab, &task, &lab.alice, "submit", json!({})).unwrap();
    let reviewing = action(&lab, &submitted, &lab.bob, "claim", json!({})).unwrap();
    action(&lab, &reviewing, &lab.bob, "approve", json!({})).unwrap();
    let labels = lab.store.project(&lab.project).unwrap()["labels"].clone();
    let label = labels[0]["uuid"].as_str().unwrap();
    assert_eq!(
        http(
            lab.store.clone(),
            Some(&lab.admin),
            "PUT",
            &format!("/api/projects/{}/labels/{label}", lab.project),
            Some(json!({"name":"robot arm"}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        lab.store
            .recording_review_counts(&lab.project, &lab.recording)
            .unwrap()["1"],
        3
    );
    assert_eq!(
        lab.store
            .review_tasks(&lab.project, &lab.alice, false)
            .unwrap()[0]["status"],
        "approved"
    );
}

#[test]
fn csv_tasks_review_saved_values_and_freeze_configuration_until_reopened() {
    let lab = lab();
    let project=lab.store.save_structured_dataset(&lab.project,&lab.recording,"2",&json!({"name":"Sensor labels","columns":[{"id":"value","name":"position","source":"/position","transform":"number"},{"id":"truth","name":"truth","default":"valid","source":null}]})).unwrap();
    let dataset = project["datasets"][0]["uuid"].as_str().unwrap();
    let created=lab.store.create_review_task(&lab.project,&lab.alice,&json!({"title":"Sensor task","recording_id":lab.recording,"dataset_id":dataset,"scopes":[{"channel_id":"2","start_frame":0,"end_frame":2}]})).unwrap();
    assert_eq!(
        lab.store
            .review_sample(
                &lab.project,
                created["id"].as_str().unwrap(),
                &lab.bob,
                "2",
                1
            )
            .unwrap()["values"]["position"],
        json!(1.0)
    );
    assert!(lab.store.dataset_csv(&lab.project, dataset).is_err());
    let submitted = action(&lab, &created, &lab.alice, "submit", json!({})).unwrap();
    let reviewing = action(&lab, &submitted, &lab.bob, "claim", json!({})).unwrap();
    let approved = action(&lab, &reviewing, &lab.bob, "approve", json!({})).unwrap();
    let approved_bytes = lab.store.dataset_csv(&lab.project, dataset).unwrap();
    assert!(!approved_bytes.is_empty());
    let made=lab.store.create_dataset_version(&lab.project,None,&lab.admin,&json!({"name":"Approved sensors","modalities":["structured"],"sources":[{"task_id":created["id"],"revision":1}]})).unwrap();
    assert_eq!(
        lab.store
            .build_dataset_version(
                &lab.project,
                made["dataset"]["uuid"].as_str().unwrap(),
                1,
                &json!({"format":"csv"})
            )
            .unwrap()["sample_count"],
        3
    );
    action(&lab, &approved, &lab.alice, "reopen", json!({})).unwrap();
    assert_eq!(
        lab.store.dataset_csv(&lab.project, dataset).unwrap(),
        approved_bytes
    );
    lab.store
        .delete_dataset_as(&lab.project, dataset, &lab.admin)
        .unwrap();
    assert_eq!(
        lab.store
            .review_revision_sample(
                &lab.project,
                created["id"].as_str().unwrap(),
                1,
                &lab.bob,
                "2",
                1
            )
            .unwrap()["values"]["position"],
        json!(1.0)
    );
}

#[test]
fn browser_number_round_trips_cannot_invalidate_a_locked_submission() {
    let lab = lab();
    let mut state =
        lab.store.recording(&lab.project, &lab.recording).unwrap()["annotations"].clone();
    state["annotations"]["1"]["0"]["score"] = json!(1.0);
    lab.store
        .save_annotations_as(&lab.project, &lab.recording, &state, &lab.alice)
        .unwrap();
    let created = task(&lab);
    let submitted = action(&lab, &created, &lab.alice, "submit", json!({})).unwrap();
    let reviewing = action(&lab, &submitted, &lab.bob, "claim", json!({})).unwrap();
    let path = lab
        .folder
        .join(format!("annotations/{}/metadata.json", lab.recording));
    let stamp = fingerprint(&path).unwrap();
    let mut browser_row = state["annotations"]["1"]["0"].clone();
    browser_row["score"] = json!(1);
    lab.store.patch_annotations_as(&lab.project,&lab.recording,&json!({"sha256":state["sha256"],"return_delta":true,"changes":[{"channel_id":"1","frame":0,"annotation":browser_row}]}),&lab.alice).unwrap();
    state["annotations"]["1"]["0"]["score"] = json!(1);
    lab.store
        .save_annotations_as(&lab.project, &lab.recording, &state, &lab.alice)
        .unwrap();
    assert_eq!(fingerprint(&path).unwrap(), stamp);
    assert_eq!(
        action(&lab, &reviewing, &lab.bob, "approve", json!({})).unwrap()["status"],
        "approved"
    );
}

#[test]
fn reviewer_assignment_is_permissioned_and_requires_starting_the_review_again() {
    let lab = lab();
    let task = task(&lab);
    let ready = action(&lab, &task, &lab.alice, "submit", json!({})).unwrap();
    assert_eq!(ready["status"], "submitted");
    assert!(ready["latest_revision"]["review_started_at"].is_null());
    assert!(action(&lab, &ready, &lab.bob, "approve", json!({})).is_err());
    assert!(action(
        &lab,
        &ready,
        &lab.bob,
        "comment",
        json!({"message":"Not started"})
    )
    .is_err());
    assert!(action(&lab, &ready, &lab.alice, "approve", json!({})).is_err());
    assert!(action(
        &lab,
        &ready,
        &lab.alice,
        "assign",
        json!({"reviewer_id":lab.bob.id})
    )
    .is_err());
    assert!(action(
        &lab,
        &ready,
        &lab.admin,
        "assign",
        json!({"reviewer_id":lab.alice.id})
    )
    .is_err());
    let outsider = user(&lab.store, &lab.admin, "outsider", "Reviewers");
    assert!(action(
        &lab,
        &ready,
        &lab.admin,
        "assign",
        json!({"reviewer_id":outsider.id})
    )
    .is_err());

    let assigned = action(
        &lab,
        &ready,
        &lab.admin,
        "assign",
        json!({"reviewer_id":lab.bob.id}),
    )
    .unwrap();
    assert_eq!(assigned["status"], "submitted");
    assert_eq!(assigned["latest_revision"]["reviewer_id"], lab.bob.id);
    assert!(action(&lab, &assigned, &lab.admin, "claim", json!({})).is_err());
    let reviewing = action(&lab, &assigned, &lab.bob, "claim", json!({})).unwrap();
    assert_eq!(reviewing["status"], "under_review");
    let reviewing = action(
        &lab,
        &reviewing,
        &lab.bob,
        "comment",
        json!({"message":"Check this section"}),
    )
    .unwrap();
    let snapshot = reviewing["latest_revision"]["snapshot"].clone();
    let ready = action(
        &lab,
        &reviewing,
        &lab.admin,
        "assign",
        json!({"reviewer_id":""}),
    )
    .unwrap();
    assert_eq!(ready["status"], "submitted");
    assert!(ready["latest_revision"]["review_started_at"].is_null());
    assert_eq!(
        ready["latest_revision"]["comments"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(ready["latest_revision"]["snapshot"], snapshot);
    assert!(action(&lab, &ready, &lab.bob, "approve", json!({})).is_err());
    let reviewing = action(&lab, &ready, &lab.bob, "claim", json!({})).unwrap();
    let approved = action(&lab, &reviewing, &lab.bob, "approve", json!({})).unwrap();
    assert_eq!(approved["status"], "approved");
    assert!(action(
        &lab,
        &approved,
        &lab.admin,
        "assign",
        json!({"reviewer_id":""})
    )
    .is_err());
}

#[test]
fn old_unstarted_reviews_are_read_as_waiting_without_rewriting_history() {
    let lab = lab();
    let task = task(&lab);
    let ready = action(&lab, &task, &lab.alice, "submit", json!({})).unwrap();
    let path = lab.folder.join(format!(
        "reviews/{}/{}/revisions/000000000001.json",
        lab.recording,
        ready["id"].as_str().unwrap()
    ));
    let mut legacy: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    legacy["status"] = json!("under_review");
    std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let before = fingerprint(&path).unwrap();
    let tasks = lab
        .store
        .review_tasks(&lab.project, &lab.bob, true)
        .unwrap();
    assert_eq!(tasks[0]["status"], "submitted");
    assert_eq!(tasks[0]["latest_revision"]["status"], "submitted");
    let revisions = lab
        .store
        .review_revisions(&lab.project, ready["id"].as_str().unwrap(), &lab.bob)
        .unwrap();
    assert_eq!(revisions[0]["status"], "submitted");
    assert_eq!(fingerprint(&path).unwrap(), before);
    let reviewing = action(&lab, &tasks[0], &lab.bob, "claim", json!({})).unwrap();
    assert_eq!(reviewing["status"], "under_review");
}

#[test]
fn reviewers_can_assign_available_revisions_to_themselves_without_assignment_management() {
    let lab = lab();
    let created = task(&lab);
    let ready = action(&lab, &created, &lab.alice, "submit", json!({})).unwrap();
    assert!(!lab.bob.can("review.assign"));
    let snapshot = ready["latest_revision"]["snapshot"].clone();
    let assigned = action(
        &lab,
        &ready,
        &lab.bob,
        "assign",
        json!({"reviewer_id":lab.bob.id}),
    )
    .unwrap();
    assert_eq!(assigned["reviewer_id"], lab.bob.id);
    assert_eq!(assigned["status"], "submitted");
    assert!(assigned["latest_revision"]["review_started_at"].is_null());
    assert_eq!(assigned["latest_revision"]["snapshot"], snapshot);
    assert!(action(
        &lab,
        &assigned,
        &lab.bob,
        "assign",
        json!({"reviewer_id":lab.admin.id})
    )
    .is_err());
    assert!(action(
        &lab,
        &assigned,
        &lab.bob,
        "assign",
        json!({"reviewer_id":""})
    )
    .is_err());
    let other = user(&lab.store, &lab.admin, "another_reviewer", "Reviewers");
    lab.store.accounts.save_members(&lab.admin,&lab.folder,&json!({"members":[{"user_id":lab.alice.id},{"user_id":lab.bob.id},{"user_id":other.id}]})).unwrap();
    assert!(action(
        &lab,
        &assigned,
        &other,
        "assign",
        json!({"reviewer_id":other.id})
    )
    .is_err());
    let reviewing = action(&lab, &assigned, &lab.bob, "claim", json!({})).unwrap();
    assert_eq!(reviewing["status"], "under_review");
    let approved = action(&lab, &reviewing, &lab.bob, "approve", json!({})).unwrap();
    assert!(action(
        &lab,
        &approved,
        &lab.bob,
        "assign",
        json!({"reviewer_id":lab.bob.id})
    )
    .is_err());
}

#[test]
fn explicit_self_assignment_allows_own_review_without_changing_policy_or_permissions() {
    let lab = lab();
    let created = task(&lab);
    let ready = action(&lab, &created, &lab.alice, "submit", json!({})).unwrap();
    let snapshot = ready["latest_revision"]["snapshot"].clone();
    assert!(action(
        &lab,
        &ready,
        &lab.alice,
        "assign",
        json!({"reviewer_id":lab.alice.id})
    )
    .is_err());
    lab.store.accounts.save_members(&lab.admin,&lab.folder,&json!({"members":[{"user_id":lab.alice.id,"permissions":["review.view"]},{"user_id":lab.bob.id}]})).unwrap();
    assert!(action(
        &lab,
        &ready,
        &lab.alice,
        "claim",
        json!({"self_review":true})
    )
    .unwrap_err()
    .to_string()
    .contains("independent reviewer"));
    assert!(action(
        &lab,
        &ready,
        &lab.admin,
        "assign",
        json!({"reviewer_id":lab.alice.id})
    )
    .is_err());
    let assigned = action(
        &lab,
        &ready,
        &lab.alice,
        "assign",
        json!({"reviewer_id":lab.alice.id}),
    )
    .unwrap();
    assert_eq!(assigned["latest_revision"]["self_review"], true);
    assert_eq!(assigned["latest_revision"]["snapshot"], snapshot);
    assert!(lab.store.accounts.settings().unwrap().independent_reviewer);
    let reviewing = action(&lab, &assigned, &lab.alice, "claim", json!({})).unwrap();
    assert!(action(&lab, &reviewing, &lab.alice, "approve", json!({})).is_err());
    lab.store.accounts.save_members(&lab.admin,&lab.folder,&json!({"members":[{"user_id":lab.alice.id,"permissions":["review.view","review.approve"]},{"user_id":lab.bob.id}]})).unwrap();
    let approved = action(&lab, &reviewing, &lab.alice, "approve", json!({})).unwrap();
    assert_eq!(approved["status"], "approved");
    assert_eq!(approved["latest_revision"]["submitted_by_id"], lab.alice.id);
    assert_eq!(approved["latest_revision"]["reviewer_id"], lab.alice.id);
    assert!(approved["history"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["action"] == "task.assign"
            && event["details"]["self_review"] == true
            && event["actor_id"] == lab.alice.id));
    assert!(lab.store.accounts.settings().unwrap().independent_reviewer);
}

#[test]
fn self_review_authorization_follows_the_submitter_and_clears_on_reassignment() {
    let lab = lab();
    lab.store.accounts.save_members(&lab.admin,&lab.folder,&json!({"members":[{"user_id":lab.alice.id,"permissions":["review.view"]},{"user_id":lab.bob.id,"permissions":["annotation.submit"]}]})).unwrap();
    let created = task(&lab);
    let first = action(&lab, &created, &lab.alice, "submit", json!({})).unwrap();
    let first = action(
        &lab,
        &first,
        &lab.alice,
        "assign",
        json!({"reviewer_id":lab.alice.id}),
    )
    .unwrap();
    let second = action(&lab, &first, &lab.alice, "submit", json!({})).unwrap();
    assert_eq!(second["latest_revision"]["self_review"], true);
    assert_eq!(second["latest_revision"]["reviewer_id"], lab.alice.id);
    let assigned = action(
        &lab,
        &second,
        &lab.admin,
        "assign",
        json!({"reviewer_id":lab.bob.id}),
    )
    .unwrap();
    assert_eq!(assigned["latest_revision"]["self_review"], false);
    assert!(action(&lab, &assigned, &lab.alice, "claim", json!({})).is_err());
    let changed_owner = action(
        &lab,
        &assigned,
        &lab.admin,
        "assign_annotator",
        json!({"annotator_id":lab.bob.id}),
    )
    .unwrap();
    let third = action(&lab, &changed_owner, &lab.bob, "submit", json!({})).unwrap();
    assert_eq!(third["latest_revision"]["self_review"], false);
    assert!(third["latest_revision"]["reviewer_id"].is_null());
    assert!(action(&lab, &third, &lab.bob, "claim", json!({})).is_err());
}
