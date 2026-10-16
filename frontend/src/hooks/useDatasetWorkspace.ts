import { useEffect, useState } from "react";
import { api, post } from "../services/http";
import type { Dataset, Project, Recording, Topic } from "../types/models";
import type { Perform } from "../types/workspace";
import { filename, recordingPath, topicCounts } from "../utils/recordings";

interface SourceChoice {
  key: string;
  recording: Recording;
  topic: Topic;
  approved: number;
  annotated: number;
}

export type DatasetWorkspaceProps = {
  project: Project;
  mode: "new" | "load" | "view" | "add";
  onModeChange: (mode: "new" | "load" | "view" | "add") => void;
  onOpenRecording: (id: string, project?: string) => Promise<void>;
  onContinueLabeling?: (id: string, project?: string) => Promise<void>;
  initialRecordings?: string[];
  onInitialRecordingsUsed?: () => void;
  busy: boolean;
  perform: Perform;
  onProject: (project: Project) => Promise<void>;
  onNotice: (notice: string) => void;
};

export function useDatasetWorkspace({
  onOpenRecording,
  project,
  mode,
  onModeChange: setMode,
  busy,
  perform,
  onProject,
  onNotice
}: DatasetWorkspaceProps) {

  const [name, setName] = useState("");
  const [dataset, setDataset] = useState<Dataset | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<Dataset | null>(null);
  const [sources, setSources] = useState<SourceChoice[]>([]);
  const [sourceErrors, setSourceErrors] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState<string[]>([]);
  const [filter, setFilter] = useState("");
  const [reload, setReload] = useState(0);
  const base = `/projects/${project.uuid}`;
  const recordingIds = project.mcaps.map((item) => item.uuid).join(",");
  useEffect(() => {
    if (mode !== "new" && mode !== "add") return;
    let active = true;
    setLoading(true);
    setSourceErrors([]);
    void (async () => {
      // Load independently: one broken recording must not hide usable annotated sources.
      const results = await Promise.allSettled(
        recordingIds
          .split(",")
          .filter(Boolean)
          .map((id) => api<Recording>(recordingPath(project.uuid, id))),
      );
      if (!active) return;
      const choices: SourceChoice[] = [];
      const errors: string[] = [];
      results.forEach((result, index) => {
        if (result.status === "rejected") {
          errors.push(
            `${filename(project.mcaps[index].source_file)}: ${String(result.reason)}`,
          );
          return;
        }
        const recording = result.value;
        recording.topics.forEach((topic) => {
          const counts = topicCounts(recording, String(topic.channel_id));
          if (topic.kind === "image" && (counts.annotated || counts.reviewed))
            choices.push({
              key: `${recording.uuid}:${topic.channel_id}`,
              recording,
              topic,
              ...counts,
              approved:counts.reviewed,
            });
        });
      });
      setSources(choices);
      setSelected((current) =>
        current.filter((key) =>
          choices.some((source) => source.key === key && source.approved > 0),
        ),
      );
      setSourceErrors(errors);
      setLoading(false);
    })();
    return () => {
      active = false;
    };
  }, [project.uuid, recordingIds, mode, reload]);

  const loadDataset = (uuid: string) =>
    void perform("Loading dataset", async () => {
      setDataset(await api<Dataset>(`${base}/datasets/${uuid}`));
      setMode("view");
      setSelected([]);
    });
  const save = () =>
    void perform(
      mode === "new" ? "Creating dataset" : "Adding annotated data",
      async () => {
        let target = dataset;
        if (mode === "new") {
          const value = await post<Project>(`${base}/datasets`, {
            name: name.trim(),
          });
          target = value.datasets.find(
            (item) =>
              !project.datasets.some((existing) => existing.uuid === item.uuid),
          )!;
          if (!target)
            throw new Error(
              "Created dataset could not be found. Load it from this project's dataset list.",
            );
          setDataset(target);
          setMode("add");
          await onProject(value);
        }
        if (!target) throw new Error("Load or create a dataset first.");
        // Keep the created dataset and remaining selection on failure so retry cannot create a second dataset.
        for (const key of selected) {
          const source = sources.find((item) => item.key === key)!;
          const value = await post<Project>(
            `${base}/datasets/${target.uuid}/entries`,
            {
              recording_uuid: source.recording.uuid,
              channel_id: String(source.topic.channel_id),
            },
          );
          setSelected((current) => current.filter((item) => item !== key));
          await onProject(value);
        }
        setDataset(await api<Dataset>(`${base}/datasets/${target.uuid}`));
        setMode("view");
        setName("");
        onNotice("Dataset saved with the selected approved annotations.");
      },
    );
  const chosen = sources.filter((item) => selected.includes(item.key));
  const visible = sources.filter((item) =>
    `${filename(item.recording.source_file)} ${item.topic.topic}`
      .toLowerCase()
      .includes(filter.toLowerCase()),
  );

  return {
    name,
    setName,
    dataset,
    setDataset,
    deleteTarget,
    setDeleteTarget,
    sources,
    sourceErrors,
    loading,
    selected,
    setSelected,
    filter,
    setFilter,
    setReload,
    base,
    loadDataset,
    save,
    chosen,
    visible,
    onOpenRecording,
    project,
    mode,
    setMode,
    busy,
    perform,
    onProject,
    onNotice
  };
}
