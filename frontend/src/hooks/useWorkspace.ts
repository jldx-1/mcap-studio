import { useCallback, useEffect, useRef, useState } from "react";
import { useAuth } from "../state/AuthContext";
import { workspaceApi } from "../services/workspace";
import type { AnnotationState, Project, ProjectSummary, Recording, RecordingSummary, Workflow } from "../types/models";
import type { Page, Perform, Theme } from "../types/workspace";
import type { ReviewNavigationTarget } from "../types/reviews";

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
      showProject(await workspaceApi.project(uuid));
    });
  };
  const openProject = async (path: string) => {
    if (busy || !(await canLeave())) return;
    await perform("Opening project", async () => {
      const value = await workspaceApi.openProject(path);
      showProject(value);
      await refreshProjects();
    });
  };
  const chooseRecording = async (uuid: string) => {
    if (!project || busy || !(await canLeave())) return;
    await perform("Opening MCAP", async () => {
      applyRecording(await workspaceApi.recording(project.uuid, uuid));
      setPage("topics");
    });
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
  const upload = async (files: File[]) => {
    if (!files.length || !project || busy) return;
    await perform("Importing MCAPs", async () => {
      let updated = project;
      // Append each successful import without changing the current section,
      // recording, filters, or annotation draft.
      for (const file of files) {
        const imported = await workspaceApi.importRecording(project.uuid, file);
        updated = { ...updated, mcaps: [...updated.mcaps, imported] };
        await updateProject(updated);
      }
      setNotice(
        `${files.length} MCAP${files.length === 1 ? "" : "s"} imported. Select one to label its topics.`,
      );
    });
    if (uploadRef.current) uploadRef.current.value = "";
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
      showProject(await workspaceApi.createProject(projectName.trim()));
      await refreshProjects();
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

  return {
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
    runProcessing
  };
}

export type WorkspaceController = ReturnType<typeof useWorkspace>;
