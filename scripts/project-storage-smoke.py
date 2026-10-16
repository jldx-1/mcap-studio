"""Run the API fixture and real PyTorch training loads in a temporary workspace."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
from urllib.request import Request, urlopen, build_opener, HTTPCookieProcessor
from http.cookiejar import CookieJar

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "python"))


def main():
    from mcap_project import Project, detection_collate
    from torch.utils.data import DataLoader

    with tempfile.TemporaryDirectory(prefix="mcap-storage-smoke-") as workspace:
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        base = f"http://127.0.0.1:{port}"
        environment = {**os.environ, "MCAP_WORKSPACE": workspace, "MCAP_BIND": f"127.0.0.1:{port}", "MCAP_API_URL": base}

        opener = build_opener(HTTPCookieProcessor(CookieJar()))

        def request(path, body=None, client=None):
            data = json.dumps(body).encode() if body is not None else None
            with (client or opener).open(Request(base + "/api" + path, data=data, headers={"Content-Type": "application/json", "X-Requested-With":"mcap-workbench"}), timeout=30) as response:
                return json.load(response)

        with (Path(workspace) / "server.log").open("w") as log:
            server = subprocess.Popen([str(ROOT / "backend/target/debug/mcap-workbench")], cwd=ROOT, env=environment, stdout=log, stderr=log)
            try:
                for attempt in range(100):
                    if server.poll() is not None:
                        raise RuntimeError((Path(workspace) / "server.log").read_text())
                    try:
                        request("/health")
                        break
                    except OSError:
                        time.sleep(0.1)
                else:
                    raise RuntimeError("Backend did not start")
                subprocess.run(["node", "scripts/api-smoke.mjs", "misc/synthetic_robot_arm.mcap"], cwd=ROOT, env=environment, check=True, timeout=120)
                request("/auth/login",{"username":"smoke-admin","password":"smoke-test-password-123"})
                projects = request("/projects")
                assert len(projects) == 1
                project = request(f"/projects/{projects[0]['uuid']}")
                prefix = f"/projects/{project['uuid']}"
                metadata = project["datasets"][0]
                for format in ("yolo", "csv"):
                    result = request(f"{prefix}/datasets/{metadata['uuid']}/build", {"format": format})
                    assert result["sample_count"] == 2
                    assert request(f"{prefix}/datasets/{metadata['uuid']}/build", {"format": format})["updated_samples"] == 0
                    source = Project.open(project["path"]).dataset(metadata["uuid"], format=format)
                    torch_source=source.to_pytorch()
                    inputs, targets = next(iter(DataLoader(torch_source, batch_size=2, num_workers=2, collate_fn=torch_source.collate_fn)))
                    assert inputs["rgb"].shape[0] == len(targets) == 2
                    assert inputs["rgb"].shape[1] == 3
                recording = project["mcaps"][0]
                detail = request(f"{prefix}/recordings/{recording['uuid']}")
                channel = next(str(topic["channel_id"]) for topic in detail["topics"] if topic["kind"] == "record")
                preview = request(f"{prefix}/recordings/{recording['uuid']}/structured/{channel}/preview", {})
                numeric = next(name for name, value in preview["rows"][0]["values"].items() if isinstance(value, (int, float)))
                project = request(f"{prefix}/recordings/{recording['uuid']}/structured/{channel}/datasets", {"name": "Sensor training", "columns": [{"id": "value", "name": "value", "source": numeric, "transform": "number"}]})
                metadata = next(item for item in project["datasets"] if item["name"] == "Sensor training")
                task = request(f"{prefix}/tasks",{"title":"Sensor training review","recording_id":recording["uuid"],"dataset_id":metadata["uuid"],"scopes":[{"channel_id":channel,"start_frame":0,"end_frame":preview["total"]-1}]})
                task = request(f"{prefix}/tasks/{task['id']}/actions",{"action":"submit","expected_version":task["version"]})
                reviewer = build_opener(HTTPCookieProcessor(CookieJar()))
                request("/auth/login",{"username":"smoke-reviewer","password":"smoke-test-password-123"},reviewer)
                task = request(f"{prefix}/tasks/{task['id']}/actions",{"action":"claim","expected_version":task["version"]},reviewer)
                request(f"{prefix}/tasks/{task['id']}/actions",{"action":"approve","expected_version":task["version"]},reviewer)
                catalog=request(f"{prefix}/approved-data")
                ready=next(s for s in catalog["sources"] if s["task_id"]==task["id"])
                modality=next(m["id"] for m in ready["modalities"] if m["channel_id"]==channel)
                published=request(f"{prefix}/dataset-versions",{"name":"Sensor training versioned","modalities":[modality],"sources":[{"task_id":task["id"],"revision":1}]})
                metadata=published["dataset"]
                request(f"{prefix}/datasets/{metadata['uuid']}/versions/1/build", {"format": "csv"})
                source = Project.open(project["path"]).dataset("Sensor training versioned",version=1).to_pytorch()
                features,targets = next(iter(DataLoader(source,batch_size=2,num_workers=2,collate_fn=source.collate_fn)))
                assert next(iter(features.values())).shape[0] == 2
                assert targets[0]["revision"] == 1
                print("Passed: version 3 API, no-op builds, real generated CSV/YOLO pages, and PyTorch image/sensor loaders with workers.")
            finally:
                server.terminate()
                try:
                    server.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait()


if __name__ == "__main__":
    main()
