"""Map-style tensors, with images opened only for the requested sample."""
from collections import OrderedDict
import json
import math

import torch
from torch.utils.data import Dataset as TorchBase

from .project import _path


class TorchDataset(TorchBase):
    def __init__(self, source, *, feature_columns=None, target_column=None, transform=None):
        self.source = source
        self.target_column = target_column
        self.transform = transform
        self.feature_columns = feature_columns
        self.multimodal = bool(len(source) and "inputs" in source[0])
        if (feature_columns is not None or target_column is not None) and len(source) and "values" in source[0]:
            self.multimodal = False
        self._records = OrderedDict()
        self._handles = OrderedDict()
        self.collate_fn = multimodal_collate if self.multimodal else legacy_collate
        if len(source) and "values" in source[0] and not self.multimodal:
            values = source[0]["values"]
            if feature_columns is None:
                self.feature_columns = [name for name in source.generated["columns"] if name != target_column and isinstance(values.get(name), (int, float))]
            if not self.feature_columns:
                raise ValueError("Select numeric feature_columns for tabular training")
            if any(name not in source.generated["columns"] for name in self.feature_columns):
                raise ValueError("Unknown feature column")
            if target_column is not None and target_column not in source.generated["columns"]:
                raise ValueError("Unknown target column")

    def __len__(self):
        return len(self.source)

    def __getitem__(self, index):
        sample = self.source[index]
        if self.multimodal:
            return self._multimodal_sample(sample)
        if "values" in sample:
            values = sample["values"]
            features = torch.tensor([values[name] for name in self.feature_columns], dtype=torch.float32)
            result = features if self.target_column is None else (features, torch.tensor(values[self.target_column]))
            return self.transform(result) if self.transform else result
        from PIL import Image
        with Image.open(_path(self.source.root, sample["image"])) as original:
            image = original.convert("RGB")
            width, height = image.size
            tensor = torch.frombuffer(bytearray(image.tobytes()), dtype=torch.uint8).reshape(height, width, 3).permute(2, 0, 1).to(torch.float32).div_(255)
        annotation = sample["annotation"]
        boxes = annotation.get("boxes", [])
        target = {
            "boxes": torch.tensor([[box["x"], box["y"], box["x"] + box["width"], box["y"] + box["height"]] for box in boxes], dtype=torch.float32).reshape(-1, 4),
            "labels": torch.tensor([box["class_id"] for box in boxes], dtype=torch.int64),
            "classifications": torch.tensor(annotation.get("labels", []), dtype=torch.int64),
            "time_ns": torch.tensor(int(sample["time_ns"]), dtype=torch.int64),
        }
        return self.transform(tensor, target) if self.transform else (tensor, target)


    def _record(self, path, offset):
        identity = (path, int(offset))
        if identity not in self._records:
            if path not in self._handles:
                self._handles[path] = _path(self.source.root, path).open("rb")
                if len(self._handles) > 8:
                    self._handles.popitem(last=False)[1].close()
            handle = self._handles[path]
            handle.seek(int(offset))
            raw = handle.readline(32 * 1024 * 1024)
            value = json.loads(raw)
            self._records[identity] = value
            if len(self._records) > 32:
                self._records.popitem(last=False)
        self._records.move_to_end(identity)
        return self._records[identity]

    def _multimodal_sample(self, sample):
        inputs = {}
        for key, descriptor in sample["inputs"].items():
            if descriptor["kind"] == "image" and "tensor" in descriptor:
                tensor = _npy_tensor(_path(self.source.root, descriptor["tensor"]["path"]))
                if tensor.ndim == 3:
                    tensor = tensor.permute(2, 0, 1)
                inputs[key] = tensor.to(torch.float32).div_(descriptor["tensor"].get("scale", 1))
            elif descriptor["kind"] == "image":
                from PIL import Image
                with Image.open(_path(self.source.root, descriptor["image"])) as original:
                    image = original.convert("RGB")
                    width, height = image.size
                    inputs[key] = torch.frombuffer(bytearray(image.tobytes()), dtype=torch.uint8).reshape(height, width, 3).permute(2, 0, 1).to(torch.float32).div_(255)
            else:
                payload = self._record(descriptor["records"], descriptor["offset"])
                flattened = [_numeric_values(value) for value in payload["values"]]
                names = self.source.generated.get("feature_schemas", {}).get(key) or sorted({name for row in flattened for name in row})
                tensor = torch.tensor([[row.get(name, float("nan")) for name in names] for row in flattened], dtype=torch.float32)
                inputs[key] = tensor if descriptor.get("window") else tensor[0]
        boxes = sample["annotation"].get("boxes", [])
        target = {
            "boxes": torch.tensor([[box["x"], box["y"], box["x"] + box["width"], box["y"] + box["height"]] for box in boxes], dtype=torch.float32).reshape(-1, 4),
            "labels": torch.tensor([box["class_id"] for box in boxes], dtype=torch.int64),
            "classifications": torch.tensor(sample["annotation"].get("labels", []), dtype=torch.int64),
            "time_ns": torch.tensor(int(sample["time_ns"]), dtype=torch.int64),
            "revision": int(sample["approved_revision"]["revision"]),
        }
        return self.transform(inputs, target) if self.transform else (inputs, target)

    def __getstate__(self):
        value = self.__dict__.copy()
        value["_handles"] = OrderedDict()
        value["_records"] = OrderedDict()
        return value

    def __del__(self):
        for handle in getattr(self, "_handles", {}).values():
            handle.close()


def _numeric_values(value, path="", result=None):
    result = {} if result is None else result
    if isinstance(value, dict):
        for key, child in value.items():
            _numeric_values(child, path + "/" + str(key), result)
    elif isinstance(value, list):
        for index, child in enumerate(value):
            _numeric_values(child, path + "/" + str(index), result)
    elif isinstance(value, (int, float)) and math.isfinite(value):
        result[path] = value
    return result


def legacy_collate(batch):
    from .project import detection_collate
    if batch and isinstance(batch[0], tuple) and isinstance(batch[0][1], dict):
        return detection_collate(batch)
    return torch.utils.data.default_collate(batch)


def multimodal_collate(batch):
    inputs, targets = zip(*batch)
    keys = sorted({key for sample in inputs for key in sample})
    combined = {}
    for key in keys:
        values = [sample.get(key) for sample in inputs]
        combined[key] = torch.stack(values) if all(isinstance(value, torch.Tensor) and value.shape == values[0].shape for value in values) else values
    return combined, list(targets)


def _npy_tensor(path):
    import ast
    import struct
    with path.open("rb") as stream:
        if stream.read(8) != b"\x93NUMPY\x01\x00":
            raise ValueError("Unsupported tensor file")
        length = struct.unpack("<H", stream.read(2))[0]
        metadata = ast.literal_eval(stream.read(length).decode("latin1"))
        if metadata["fortran_order"]:
            raise ValueError("Fortran-order tensors are unsupported")
        dtype = {"|u1":torch.uint8,"<u2": torch.uint16, "<f4": torch.float32, "<f8": torch.float64}.get(metadata["descr"])
        if dtype is None:
            raise ValueError("Unsupported tensor dtype")
        return torch.frombuffer(bytearray(stream.read()), dtype=dtype).reshape(metadata["shape"])
