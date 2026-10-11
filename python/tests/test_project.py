import importlib.util
import json
import pickle
from pathlib import Path
import tempfile
import unittest

from mcap_project import Project, detection_collate
from mcap_project.project import _path


PROJECT_ID = "00000000-0000-4000-8000-000000000001"
DATASET_ID = "00000000-0000-4000-8000-000000000002"


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value), encoding="utf-8")


def fixture(root, *, image=False, count=513):
    format = "yolo" if image else "csv"
    write(root / "project.json", {"kind": "mcap-labeling-project", "format_version": 3, "uuid": PROJECT_ID, "mcaps": {}, "labels": {}, "datasets": {DATASET_ID: {"metadata": f"datasets/{DATASET_ID}/dataset.json"}}})
    write(root / f"datasets/{DATASET_ID}/dataset.json", {"uuid": DATASET_ID, "name": "training", "format": format})
    base = f"datasets/{DATASET_ID}/generated/{format}"
    pages = {}
    for page in range((count + 255) // 256):
        rows = {}
        for index in range(page * 256, min(count, (page + 1) * 256)):
            sample = {"key": str(index), "frame": index, "time_ns": "1791057600000000001"}
            if image:
                sample.update(image=f"{base}/images/{index}.png", annotation={"boxes": [{"x": 0, "y": 0, "width": 1, "height": 1, "class_id": 0}] if index == 0 else [], "labels": []})
            else:
                sample["values"] = {"position": index, "target": index + 1, "description": "robot"}
            rows[str(index)] = sample
        relative = f"{base}/samples/{page}.json"
        write(root / relative, rows)
        pages[str(page)] = {"path": relative, "count": len(rows)}
    write(root / f"{base}/manifest.json", {"format_version": 1, "dataset_uuid": DATASET_ID, "project_uuid": PROJECT_ID, "format": format, "sample_count": count, "sample_pages": pages, "columns": ["position", "target", "description"]})
    return root / base


class ProjectTests(unittest.TestCase):
    def test_lazy_pages_boundaries_and_worker_pickle(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture(root)
            dataset = Project.open(root).dataset("training")
            self.assertEqual(len(dataset), 513)
            self.assertEqual(len(dataset._cache), 0)
            self.assertEqual(dataset[256]["values"]["position"], 256)
            self.assertEqual(len(dataset._cache), 1)
            self.assertEqual(dataset[-1]["frame"], 512)
            sample = dataset[256]
            sample["values"]["position"] = -1
            self.assertEqual(dataset[256]["values"]["position"], 256)
            restored = pickle.loads(pickle.dumps(dataset))
            self.assertEqual(len(restored._cache), 0)
            self.assertEqual(restored[0]["frame"], 0)
            with self.assertRaises(IndexError):
                dataset[513]

    def test_cache_is_bounded(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture(Path(directory), count=256 * 10)
            dataset = Project.open(directory).dataset(DATASET_ID)
            for index in range(0, len(dataset), 256):
                dataset[index]
            self.assertEqual(len(dataset._cache), 8)

    def test_rejects_path_traversal_and_symlinks(self):
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryDirectory() as outside:
            root = Path(directory)
            for relative in ("../outside", "/tmp/file", "some/../file", "some\\..\\file"):
                with self.assertRaises(ValueError):
                    _path(root, relative)
            (root / "escape").symlink_to(outside, target_is_directory=True)
            with self.assertRaises(ValueError):
                _path(root, "escape/data.json")

    def test_missing_build_is_actionable(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture(root, count=1)
            (root / f"datasets/{DATASET_ID}/generated/csv/manifest.json").unlink()
            with self.assertRaisesRegex(FileNotFoundError, "Generate this dataset"):
                Project.open(root).dataset("training")

    def test_corrupt_counts_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            base = fixture(root, count=1)
            manifest = json.loads((base / "manifest.json").read_text())
            manifest["sample_count"] = 2
            write(base / "manifest.json", manifest)
            with self.assertRaises(ValueError):
                Project.open(root).dataset("training")

    @unittest.skipUnless(importlib.util.find_spec("torch"), "PyTorch optional dependency unavailable")
    def test_tabular_dataloader_with_workers(self):
        from torch.utils.data import DataLoader
        with tempfile.TemporaryDirectory() as directory:
            fixture(Path(directory), count=4)
            source = Project.open(directory).dataset("training")
            dataset = source.to_pytorch(feature_columns=["position"], target_column="target")
            batch = next(iter(DataLoader(dataset, batch_size=2, num_workers=2)))
            self.assertEqual(batch[0].tolist(), [[0.0], [1.0]])
            self.assertEqual(batch[1].tolist(), [1, 2])

    @unittest.skipUnless(importlib.util.find_spec("torch") and importlib.util.find_spec("PIL"), "PyTorch/Pillow optional dependencies unavailable")
    def test_image_loader_is_lazy_and_handles_negative_frames(self):
        from PIL import Image
        from torch.utils.data import DataLoader
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            base = fixture(root, image=True, count=2)
            dataset = Project.open(root).dataset("training").to_pytorch()
            # Construction succeeds while images are absent; access performs the I/O.
            (base / "images").mkdir()
            for index in range(2):
                Image.new("RGB", (2, 2), "red").save(base / f"images/{index}.png")
            images, targets = next(iter(DataLoader(dataset, batch_size=2, num_workers=2, collate_fn=detection_collate)))
            self.assertEqual(tuple(images[0].shape), (3, 2, 2))
            self.assertEqual(targets[0]["boxes"].tolist(), [[0.0, 0.0, 1.0, 1.0]])
            self.assertEqual(tuple(targets[1]["boxes"].shape), (0, 4))
            self.assertEqual(targets[0]["time_ns"].item(), 1791057600000000001)


if __name__ == "__main__":
    unittest.main()
