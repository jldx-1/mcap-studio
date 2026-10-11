"""Read project source files and generated sample pages without a server."""
from __future__ import annotations

from bisect import bisect_right
from collections import OrderedDict
import json
from pathlib import Path
from typing import Iterator


def _path(root: Path, relative: str) -> Path:
    candidate = Path(relative)
    if not relative or candidate.is_absolute() or any(part in (".", "..") for part in relative.replace("\\", "/").split("/")):
        raise ValueError("Project paths must be relative and stay inside their folder")
    path = (root / candidate).resolve()
    if path == root or not path.is_relative_to(root):
        raise ValueError("Project path escapes its folder")
    return path


def _json(path: Path):
    with path.open(encoding="utf-8") as stream:
        return json.load(stream)


class Project:
    def __init__(self, path):
        self.path = Path(path).resolve()
        if self.path.is_file():
            self.path = self.path.parent
        self.metadata = _json(_path(self.path, "project.json"))
        if self.metadata.get("kind") != "mcap-labeling-project" or self.metadata.get("format_version") not in (1, 2, 3):
            raise ValueError("Unsupported MCAP project")

    @classmethod
    def open(cls, path) -> Project:
        return cls(path)

    def _items(self, registry: str) -> Iterator[dict]:
        for uuid, item in self.metadata.get(registry, {}).items():
            if self.metadata["format_version"] == 3:
                item = _json(_path(self.path, item["metadata"]))
            if item["uuid"] != uuid:
                raise ValueError("Metadata UUID does not match registry")
            yield item

    def mcaps(self) -> Iterator[dict]:
        return self._items("mcaps")

    def labels(self) -> Iterator[dict]:
        return self._items("labels")

    def datasets(self) -> Iterator[dict]:
        return self._items("datasets")

    def annotations(self, recording_uuid: str) -> Iterator[dict]:
        item = next((item for item in self.mcaps() if item["uuid"] == recording_uuid), None)
        if item is None:
            raise KeyError(recording_uuid)
        if self.metadata["format_version"] == 3:
            state_path = _path(self.path, f"annotations/{recording_uuid}/metadata.json")
            if not state_path.exists():
                return
            state = _json(state_path)
            for page in state["pages"].values():
                for key, row in _json(_path(self.path, page["path"])).items():
                    channel, frame = key.split(":")
                    yield {"channel_id": channel, "frame": int(frame), "annotation": row}
        else:
            state_path = _path(self.path, str(Path(item["path"]).parent / "annotations.json"))
            if not state_path.exists():
                return
            state = _json(state_path)
            for channel, rows in state["annotations"].items():
                for frame, row in rows.items():
                    yield {"channel_id": channel, "frame": int(frame), "annotation": row}

    def dataset(self, name_or_uuid: str, format: str | None = None, *, version: int | None = None) -> Dataset:
        metadata = next((item for item in self.datasets() if name_or_uuid in (item["name"], item["uuid"])), None)
        if metadata is None:
            raise KeyError(f"Dataset not found: {name_or_uuid}")
        if metadata.get("kind") == "approved-dataset":
            number = int(version if version is not None else metadata["latest_version"])
            selected = next((item for item in metadata["versions"] if item["number"] == number), None)
            if selected is None:
                raise KeyError(f"Dataset version not found: {number}")
            definition = _json(_path(self.path, selected["path"]))
            if definition.get("dataset_uuid") != metadata["uuid"] or definition.get("number") != number:
                raise ValueError("Dataset version identity does not match")
            format = format or definition["default_format"]
            if format not in ("csv", "yolo"):
                raise ValueError(f"Unsupported generated format: {format}")
            relative = f"datasets/{metadata['uuid']}/versions/{number:012d}/generated/{format}/manifest.json"
            manifest = _path(self.path, relative)
            if not manifest.is_file():
                raise FileNotFoundError(f"Build {format.upper()} for dataset version {number} before loading it")
            generated = _json(manifest)
            if generated.get("dataset_version") != number:
                raise ValueError("Generated dataset version does not match")
            if selected.get("sha256") and generated.get("source_version_sha256") != selected["sha256"]:
                raise ValueError("Generated dataset provenance does not match its pinned definition")
            metadata = {**metadata, "version": number, "definition": definition}
            return Dataset(self.path, metadata, generated)
        if version is not None:
            raise ValueError("This legacy dataset has no pinned versions")
        format = format or metadata.get("format", "yolo")
        if format not in ("csv", "yolo"):
            raise ValueError(f"Unsupported generated format: {format}")
        relative = f"datasets/{metadata['uuid']}/generated/{format}/manifest.json"
        manifest = _path(self.path, relative)
        if not manifest.is_file():
            raise FileNotFoundError("Generate this dataset with the application's Build dataset action or POST /datasets/{uuid}/build first")
        return Dataset(self.path, metadata, _json(manifest))


class Dataset:
    """Random access with at most eight source pages held in memory per worker."""
    def __init__(self, root: Path, metadata: dict, generated: dict):
        self.root = root
        self.metadata = metadata
        self.generated = generated
        if generated.get("format_version") != 1 or generated.get("dataset_uuid") != metadata["uuid"]:
            raise ValueError("Generated dataset does not match source metadata")
        self._pages = list(generated["sample_pages"].values())
        self._ends = []
        count = 0
        for page in self._pages:
            count += page["count"]
            self._ends.append(count)
        if count != generated["sample_count"]:
            raise ValueError("Generated sample counts do not match")
        self._cache: OrderedDict[int, list] = OrderedDict()

    @property
    def version(self):
        return self.metadata.get("version")

    @property
    def provenance(self):
        return self.generated.get("source_revisions", [])

    def __len__(self):
        return self.generated["sample_count"]

    def __getitem__(self, index: int):
        if index < 0:
            index += len(self)
        if not 0 <= index < len(self):
            raise IndexError(index)
        page_index = bisect_right(self._ends, index)
        offset = index - (self._ends[page_index - 1] if page_index else 0)
        if page_index not in self._cache:
            rows = list(_json(_path(self.root, self._pages[page_index]["path"])).values())
            if len(rows) != self._pages[page_index]["count"]:
                raise ValueError("Sample page count does not match")
            self._cache[page_index] = rows
            if len(self._cache) > 8:
                self._cache.popitem(last=False)
        self._cache.move_to_end(page_index)
        # Callers may transform a sample without changing the cached source page.
        from copy import deepcopy
        return deepcopy(self._cache[page_index][offset])

    def samples(self) -> Iterator[dict]:
        for index in range(len(self)):
            yield self[index]

    def to_pytorch(self, *, feature_columns=None, target_column=None, transform=None):
        from .torch_dataset import TorchDataset
        return TorchDataset(self, feature_columns=feature_columns, target_column=target_column, transform=transform)

    def __getstate__(self):
        state = self.__dict__.copy()
        state["_cache"] = OrderedDict()
        return state


def detection_collate(batch):
    """Keep variable-size images and bounding boxes as lists in each batch."""
    images, targets = zip(*batch)
    return list(images), list(targets)
