"""Portable MCAP projects. PyTorch is imported only by Dataset.to_pytorch()."""
from .project import Dataset, Project, detection_collate

__all__ = ["Project", "Dataset", "detection_collate"]
