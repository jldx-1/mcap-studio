import importlib.util
import json
from pathlib import Path
import pickle
import tempfile
import unittest

from mcap_project import Project
from test_project import write, PROJECT_ID, DATASET_ID


def version_fixture(root, number, values=(1000, 2000, 3000, 4000)):
    from PIL import Image
    import numpy as np
    base = f"datasets/{DATASET_ID}/versions/{number:012d}"
    generated = f"{base}/generated/csv"
    image = root / generated / "rgb.png"
    image.parent.mkdir(parents=True, exist_ok=True)
    Image.new("RGB", (2, 2), "red").save(image)
    np.save(root / generated / "depth.npy", np.array(values, dtype=np.uint16).reshape(2, 2))
    (root / generated / "imu.jsonl").write_text(json.dumps({"values": [{"acceleration": {"x": number, "y": 2, "z": 3}}]}) + "\n")
    sample = {"key": "0", "time_ns": "1791057600000000001", "approved_revision": {"revision": number}, "annotation": {"boxes": [], "labels": [0]}, "inputs": {
        "rgb": {"kind": "image", "image": f"{generated}/rgb.png"},
        "depth": {"kind": "image", "tensor": {"path": f"{generated}/depth.npy", "scale": 1}},
        "imu": {"kind": "record", "records": f"{generated}/imu.jsonl", "offset": 0},
    }}
    page = f"{generated}/samples/0.json"
    write(root / page, {"0": sample})
    write(root / f"{base}/version.json", {"dataset_uuid": DATASET_ID, "number": number, "default_format": "csv"})
    write(root / f"{generated}/manifest.json", {"format_version": 1, "dataset_uuid": DATASET_ID, "dataset_version": number, "sample_count": 1, "sample_pages": {"0": {"path": page, "count": 1}}, "columns": [], "source_revisions": [{"revision": number}], "feature_schemas": {"imu": ["/acceleration/x", "/acceleration/y", "/acceleration/z"]}})
    return {"number": number, "path": f"{base}/version.json"}


@unittest.skipUnless(all(importlib.util.find_spec(name) for name in ("torch", "PIL", "numpy")), "training dependencies are optional")
class VersionTests(unittest.TestCase):
    def test_explicit_version_stays_pinned_when_latest_changes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            versions = [version_fixture(root, 1), version_fixture(root, 2)]
            write(root / "project.json", {"kind": "mcap-labeling-project", "format_version": 3, "uuid": PROJECT_ID, "datasets": {DATASET_ID: {"metadata": f"datasets/{DATASET_ID}/dataset.json"}}})
            write(root / f"datasets/{DATASET_ID}/dataset.json", {"uuid": DATASET_ID, "name": "training", "kind": "approved-dataset", "latest_version": 2, "versions": versions})
            project = Project.open(root)
            old = project.dataset("training", version=1)
            self.assertEqual(old.version, 1)
            self.assertEqual(project.dataset("training").version, 2)
            self.assertEqual(old.provenance[0]["revision"], 1)
            self.assertEqual(len(old._cache), 0)
            self.assertEqual(old[0]["approved_revision"]["revision"], 1)
            with self.assertRaises(KeyError):
                project.dataset("training", version=3)

    def test_multimodal_loader_preserves_depth_and_supports_worker_processes(self):
        import torch
        from torch.utils.data import DataLoader
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            versions = [version_fixture(root, 1)]
            write(root / "project.json", {"kind": "mcap-labeling-project", "format_version": 3, "uuid": PROJECT_ID, "datasets": {DATASET_ID: {"metadata": f"datasets/{DATASET_ID}/dataset.json"}}})
            write(root / f"datasets/{DATASET_ID}/dataset.json", {"uuid": DATASET_ID, "name": "training", "kind": "approved-dataset", "latest_version": 1, "versions": versions})
            dataset = Project.open(root).dataset("training").to_pytorch()
            self.assertEqual(len(dataset._handles), 0)
            inputs, target = dataset[0]
            self.assertEqual(tuple(inputs["rgb"].shape), (3, 2, 2))
            self.assertTrue(torch.equal(inputs["depth"], torch.tensor([[1000., 2000.], [3000., 4000.]])))
            self.assertTrue(torch.equal(inputs["imu"], torch.tensor([1., 2., 3.])))
            self.assertEqual(target["revision"], 1)
            restored = pickle.loads(pickle.dumps(dataset))
            self.assertEqual(len(restored._handles), 0)
            loaded, targets = next(iter(DataLoader(restored, batch_size=1, num_workers=2, collate_fn=restored.collate_fn)))
            self.assertEqual(tuple(loaded["depth"].shape), (1, 2, 2))
            self.assertEqual(targets[0]["time_ns"].item(), 1791057600000000001)
