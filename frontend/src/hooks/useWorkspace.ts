import { useCallback, useEffect, useRef, useState } from "react";
import { useAuth } from "../state/AuthContext";
import { workspaceApi } from "../services/workspace";
import type { AnnotationState, Project, ProjectSummary, Recording, RecordingSummary, Workflow } from "../types/models";
import type { Page, Perform, Theme } from "../types/workspace";
import type { ReviewNavigationTarget } from "../types/reviews";
import { can } from "../utils/permissions";
import { readTranslationFiles } from "../components/processing/RecordingPreparation";
import type { PreparationJob, TranslationProfile } from "../types/preparation";

export function useWorkspace() {
  const auth = useAuth();

  const [theme, setTheme] = useState<Theme>(
    () => (localStorage.getItem("mcap-theme") as Theme) || "system",
  );
  const [page, setPage] = useState<Page>("projects");
  const [reviewTarget, setReviewTarget] = useState<ReviewNavigationTarget | null>(null);
  const [sidebar, setSidebar] = useState(false);
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [project, setProject] = useState<Project | null>(null);
  const [recording, setRecording] = useState<Recording | null>(null);
  const [channel, setChannel] = useState("");
  const [connected, setConnected] = useState<boolean | null>(null);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [dialog, setDialog] = useState<
    "create" | "open" | "dataset" | "labels" | "export" | null
  >(null);
  const [projectName, setProjectName] = useState("");
  const [search, setSearch] = useState("");
  const [dateSort, setDateSort] = useState("newest");
  const [removeTarget, setRemoveTarget] = useState<RecordingSummary | null>(null);
  const [datasetStart, setDatasetStart] = useState<"new" | "load" | "view" | "add">("load");
  const [workflow, setWorkflow] = useState<Workflow | null>(null);
  const uploadRef = useRef<HTMLInputElement>(null);
  const [pendingUploads, setPendingUploads] = useState<File[]>([]);
  const [preparationRecordings, setPreparationRecordings] = useState<RecordingSummary[]>([]);
  const [translationFiles, setTranslationFiles] = useState<TranslationProfile[]>([]);
  const [datasetRecordingIds, setDatasetRecordingIds] = useState<string[]>([]);
  const [labelingQueue, setLabelingQueue] = useState<string[]>([]);
  const [pendingLabelingJobs, setPendingLabelingJobs] = useState<string[]>([]);
  const [importProgress, setImportProgress] = useState("");
  const queueKey = project && auth.user ? `mcap-labeling-queue/${auth.user.id}/${project.uuid}` : null;
  useEffect(() => {
    if (!queueKey) return;
    try { setLabelingQueue(JSON.parse(localStorage.getItem(queueKey) || "[]")); } catch { setLabelingQueue([]); }
    try { setPendingLabelingJobs(JSON.parse(localStorage.getItem(`${queueKey}/jobs`) || "[]")); } catch { setPendingLabelingJobs([]); }
  }, [queueKey]);
  const saveLabelingQueue = (ids: string[]) => { setLabelingQueue(ids); if (queueKey) localStorage.setItem(queueKey, JSON.stringify(ids)); };
  const importing = useRef(false);
  const navigationGuard = useRef<(() => Promise<boolean>) | null>(null);
  const registerNavigationGuard = useCallback(
    (guard: (() => Promise<boolean>) | null) => {
      navigationGuard.current = guard;
    },
    [],
  );
  const canLeave = async () =>
    !navigationGuard.current || (await navigationGuard.current());

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      document.documentElement.dataset.theme =
        theme === "system" ? (media.matches ? "dark" : "light") : theme;
    };
    apply();
    localStorage.setItem("mcap-theme", theme);
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);
  useEffect(() => {
    if (!notice) return;
    const id = window.setTimeout(() => setNotice(""), 6500);
    return () => window.clearTimeout(id);
  }, [notice]);
  const perform = useCallback<Perform>(async (label, action) => {
    setBusy(label);
    setError("");
    try {
      await action();
      return true;
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      return false;
    } finally {
      setBusy("");
    }
  }, []);
  const refreshProjects = useCallback(async () => {
    setProjects(await workspaceApi.listProjects());
  }, []);
  const connect = useCallback(async () => {
    try {
      await workspaceApi.health();
    } catch (err) {
      setConnected(false);
      setError(err instanceof Error ? err.message : String(err));
      return;
    }
    setConnected(true);
    setError("");
    try {
      await refreshProjects();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [refreshProjects]);
  useEffect(() => {
    void connect();
  }, [connect]);
  useEffect(() => {
    if (connected !== false) return;
    const timer = window.setInterval(() => void connect(), 2500);
    return () => window.clearInterval(timer);
  }, [connected, connect]);

  const applyRecording = (value: Recording) => {
    setRecording(value);
    setChannel("");
    setWorkflow(
      value.workflow || {
        selected_channels: value.topics.map((topic) =>
          String(topic.channel_id),
        ),
        translations: {},
        sync: {
          enabled: false,
          parent_channel: "",
          mode: "nearest",
          tolerance_ns: null,
          delete_out_of_sync: false,
        },
      },
    );
  };
  const showProject = (value: Project) => {
    setReviewTarget(null);
    setProject(value);
    setRecording(null);
    setChannel("");
    setWorkflow(null);
    setPage("project");
    setSearch("");
    setSidebar(false);
    setDialog(null);
  };
  const loadProject = async (uuid: string) => {
    if (busy || !(await canLeave())) return;
    await perform("Loading project", async () => {
      const value = await workspaceApi.project(uuid);
      showProject(value);
      await importPendingFiles(value);
    });
  };
  const openProject = async (path: string) => {
    if (busy || !(await canLeave())) return;
    await perform("Opening project", async () => {
      const value = await workspaceApi.openProject(path);
      showProject(value);
      await refreshProjects();
      await importPendingFiles(value);
    });
  };
  const chooseRecording = async (uuid: string, sourceProject?: string) => {
    if (!project || busy || !(await canLeave())) return;
    await perform("Opening MCAP", async () => {
      const projectId = sourceProject || project.uuid;
      if (projectId !== project.uuid) showProject(await workspaceApi.project(projectId));
      applyRecording(await workspaceApi.recording(projectId, uuid));
      setPage("topics");
    });
  };
  const startLabeling = async (uuid: string, ids = labelingQueue, sourceProject?: string) => {
    if (!project || busy || !(await canLeave())) return;
    await perform("Opening labeling queue", async () => {
      const projectId = sourceProject || project.uuid;
      if (projectId !== project.uuid) showProject(await workspaceApi.project(projectId));
      const value = await workspaceApi.recording(projectId, uuid);
      applyRecording(value);
      const candidates = value.topics.filter(t => !value.workflow || value.workflow.selected_channels.includes(String(t.channel_id)));
      const topic = candidates.find(t => value.annotations.techniques[String(t.channel_id)] && value.annotations.techniques[String(t.channel_id)] !== "undefined") || candidates.find(t => t.kind === "image") || candidates[0];
      if (topic) { setChannel(String(topic.channel_id)); setPage(topic.kind === "image" ? "labeling" : "structured"); }
      else setPage("topics");
      setLabelingQueue(ids);
      localStorage.setItem(`mcap-labeling-queue/${auth.user?.id}/${projectId}`, JSON.stringify(ids));
      localStorage.setItem(`mcap-last-labeling/${auth.user?.id}/${projectId}`, uuid);
    });
  };
  const prepareRecordings = async (ids: string[]) => {
    if (!project || busy || !(await canLeave())) return;
    setPreparationRecordings(project.mcaps.filter(r => ids.includes(r.uuid)));
    setPage("prepare");
  };
  const addRecordingsToDataset = async (ids: string[]) => {
    if (busy || !(await canLeave())) return;
    setDatasetRecordingIds(ids); setDatasetStart("new"); setPage("datasets");
  };
  const preparationQueued = (jobs: PreparationJob[], start: boolean) => {
    if (start) {
      const ids = [...new Set([...pendingLabelingJobs, ...jobs.map(j => j.uuid)])];
      setPendingLabelingJobs(ids);
      if (queueKey) localStorage.setItem(`${queueKey}/jobs`, JSON.stringify(ids));
    }
    setPreparationRecordings([]); setPage("mcaps");
  };
  const resolveLabelingJobs = (jobs: PreparationJob[]) => {
    const ready = jobs.filter(j => pendingLabelingJobs.includes(j.uuid) && j.status === "ready" && j.output_recording);
    if (!ready.length) return;
    saveLabelingQueue([...new Set([...labelingQueue, ...ready.map(j => j.output_recording!)])]);
    const remaining = pendingLabelingJobs.filter(id => !ready.some(j => j.uuid === id));
    setPendingLabelingJobs(remaining);
    if (queueKey) localStorage.setItem(`${queueKey}/jobs`, JSON.stringify(remaining));
  };
  const serverImported = async (items: RecordingSummary[]) => {
    if (!project) return;
    await updateProject(await workspaceApi.project(project.uuid));
    setPreparationRecordings(items); setPage("prepare");
  };
  const nextLabeling = async () => {
    if (!recording || !project || !(await canLeave())) return;
    const topics = recording.topics.filter(t => t.frame_count > 0 && (t.kind === "image" || ["csv", "classification", "yolo"].includes(recording.annotations.techniques[String(t.channel_id)])));
    const next = topics[topics.findIndex(t => String(t.channel_id) === channel) + 1];
    if (next) { setChannel(String(next.channel_id)); setPage(next.kind === "image" ? "labeling" : "structured"); return; }
    const remaining = labelingQueue.filter(id => id !== recording.uuid);
    saveLabelingQueue(remaining);
    if (remaining.length) await startLabeling(remaining[0], remaining);
    else { setNotice("Labeling queue finished. Submit completed revisions for review, then add them to your dataset."); setPage("mcaps"); }
  };
  const go = async (value: Page) => {
    if (busy || value === page || !(await canLeave())) return;
    if (project && ["project", "mcaps", "labels"].includes(value)) {
      const ok = await perform("Refreshing project", async () => {
        setProject(await workspaceApi.project(project.uuid));
      });
      if (!ok) return;
    }
    if (["project", "projects", "mcaps", "labels", "datasets", "administration", "account", "permissions", "members", "reviews", "review"].includes(value)) {
      setRecording(null);
      setChannel("");
      setWorkflow(null);
    }
    if (value === "projects") {
      setProject(null);
      setSearch("");
    }
    setSearch("");
    setReviewTarget(null);
    setPage(value);
    setSidebar(false);
  };
  const openReview = async (target: ReviewNavigationTarget) => {
    if (busy || !(await canLeave())) return;
    setReviewTarget(target);
    setRecording(null);
    setChannel("");
    setWorkflow(null);
    setPage("review");
    setSidebar(false);
  };
  const updateProject = async (value: Project) => {
    setProject((previous) => ({ ...value,permissions:value.permissions || previous?.permissions || auth.user?.permissions }));
    const summary: ProjectSummary = { uuid: value.uuid, name: value.name, path: value.path, recording_count: value.mcaps.length, label_count: value.labels.length, dataset_count: value.datasets.length };
    setProjects((current) => current.some((item) => item.uuid === value.uuid)
      ? current.map((item) => item.uuid === value.uuid ? summary : item)
      : [...current, summary]);
  };
  const importFiles = async (files: File[], destination: Project) => {
    if (!can(destination, auth.user, "project.manage")) {
      throw new Error("Choose a project you can manage to import MCAP files.");
    }
    const profiles = await readTranslationFiles(files.filter(f => /\.(json|txt)$/i.test(f.name)));
    if (profiles.length) setTranslationFiles(profiles);
    const mcaps = files.filter(f => /\.mcap$/i.test(f.name));
    const importedRecordings: RecordingSummary[] = [];
    const failures: string[] = [];
    let duplicates = 0;
    // Append successful imports without changing the current section,
    // recording, filters, or annotation draft.
    for (const [index, file] of mcaps.entries()) {
      setImportProgress(`Uploading ${index + 1} of ${mcaps.length}: ${file.name}`);
      try {
        const imported = await workspaceApi.importRecording(destination.uuid, file);
        importedRecordings.push(imported);
        if (imported.duplicate) duplicates++;
        setPendingUploads(current => current.filter(pending => pending !== file));
      } catch (err) {
        failures.push(`${file.name}: ${err instanceof Error ? err.message : String(err)}`);
        setError(failures.join("\n"));
      }
    }
    setImportProgress("");
    if (importedRecordings.length) {
      try {
        await updateProject(await workspaceApi.project(destination.uuid));
      } catch (err) {
        throw new Error([...failures, `Could not refresh the project: ${err instanceof Error ? err.message : String(err)}`].join("\n"));
      }
    }
    setPendingUploads(current => current.filter(f => !profiles.length || !/\.(json|txt)$/i.test(f.name)));
    if (importedRecordings.length) {
      setPreparationRecordings([...new Map(importedRecordings.map(r => [r.uuid, r])).values()]);
      if (!["labeling", "structured", "review"].includes(page)) setPage("prepare");
    }
    if (importedRecordings.length || profiles.length) {
    setNotice(`${importedRecordings.length} recordings ready to prepare${duplicates ? ` · ${duplicates} existing recordings reused` : ""}${profiles.length ? ` · ${profiles.length} translations loaded` : ""}.`);
    } else {
      setNotice("");
    }
    if (failures.length) setError(failures.join("\n"));
  };
  const importPendingFiles = async (destination: Project) => {
    if (!pendingUploads.length) return;
    setBusy("Importing MCAPs");
    await importFiles(pendingUploads, destination);
  };
  const upload = async (files: File[]) => {
    if (uploadRef.current) uploadRef.current.value = "";
    if (!files.length) return;
    if (busy || importing.current) {
      setError("Wait for the current task to finish before importing MCAP files.");
      return;
    }
    if (connected !== true) {
      setError("Connect to the backend before importing MCAP files.");
      return;
    }
    if (files.some(file => !/\.(mcap|json|txt)$/i.test(file.name))) {
      setError("Drop .mcap recordings together with .json or .txt translation files.");
      return;
    }
    if (!project) {
      setError("");
      setPendingUploads((current) => [...current, ...files]);
      return;
    }
    importing.current = true;
    try {
      await perform("Importing MCAPs", () => importFiles(files, project));
    } finally {
      importing.current = false;
    }
  };
  const onSaved = useCallback((value: AnnotationState) => {
    setRecording((current) =>
      current ? { ...current, annotations: value } : current,
    );
  }, []);
  const openDialog = (
    kind: "create" | "open" | "dataset" | "labels" | "export",
  ) => {
    setError("");
    setDialog(kind);
  };

  const createProject = async () => {
    if (!(await canLeave())) return;
    await perform("Creating project", async () => {
      const value = await workspaceApi.createProject(projectName.trim());
      showProject(value);
      await refreshProjects();
      await importPendingFiles(value);
    });
  };
  const removeRecording = async () => {
    if (!project || !removeTarget) return;
    await perform("Removing MCAP", async () => {
      await updateProject(await workspaceApi.deleteRecording(project.uuid, removeTarget.uuid));
      setRemoveTarget(null);
      setNotice("MCAP removed from project.");
    });
  };
  const saveProcessing = async () => {
    if (!project || !recording || !workflow) return;
    await perform("Saving pipeline", async () => {
      await workspaceApi.saveWorkflow(project.uuid, recording.uuid, workflow);
      setNotice("Pipeline configuration saved.");
    });
  };
  const runProcessing = async () => {
    if (!project || !recording || !workflow) return;
    await perform("Running pipeline", async () => {
      applyRecording(await workspaceApi.runPipeline(project.uuid, recording.uuid, workflow));
      await updateProject(await workspaceApi.project(project.uuid));
      setPage("topics");
      setNotice("Pipeline finished. Your output MCAP is ready.");
    });
  };
  const applyTranslations = async (selections: Record<string,string>) => {
    if (!project || !recording || busy || !(await canLeave())) return;
    await perform("Applying topic translations",async()=>{
      const output = await workspaceApi.applyTranslations(project.uuid,recording.uuid,selections);
      applyRecording(output);
      await updateProject(await workspaceApi.project(project.uuid));
      setPage("topics");
      setNotice("Translations applied. The processed MCAP is ready to label.");
    });
  };

  return {
    preparationRecordings, translationFiles, prepareRecordings, preparationQueued, importProgress,
    datasetRecordingIds, clearDatasetRecordingIds: useCallback(() => setDatasetRecordingIds([]), []),
    addRecordingsToDataset, labelingQueue, startLabeling, saveLabelingQueue, resolveLabelingJobs,
    serverImported, nextLabeling,
    reviewTarget,
    openReview,
    user: auth.user,
    signOut: async () => { if(await canLeave()) await perform("Signing out",auth.signOut); },
    theme,
    setTheme,
    page,
    setPage,
    sidebar,
    setSidebar,
    projects,
    project,
    recording,
    setRecording,
    channel,
    setChannel,
    connected,
    busy,
    error,
    setError,
    notice,
    setNotice,
    dialog,
    setDialog,
    projectName,
    setProjectName,
    search,
    setSearch,
    dateSort,
    setDateSort,
    removeTarget,
    setRemoveTarget,
    datasetStart,
    setDatasetStart,
    workflow,
    setWorkflow,
    uploadRef,
    pendingUploads,
    clearPendingUploads: () => setPendingUploads([]),
    registerNavigationGuard,
    canLeave,
    perform,
    refreshProjects,
    connect,
    applyRecording,
    showProject,
    loadProject,
    openProject,
    chooseRecording,
    go,
    updateProject,
    upload,
    onSaved,
    openDialog,
    createProject,
    removeRecording,
    saveProcessing,
    runProcessing,
    applyTranslations
  };
}

export type WorkspaceController = ReturnType<typeof useWorkspace>;
