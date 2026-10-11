import { ArrowLeft, ArrowRight, FolderOpen, LoaderCircle } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { post } from "../../services/http";
import Dialog from "../shared/Dialog";

interface FolderListing {
  path: string;
  parent: string | null;
  is_project: boolean;
  folders: { name: string; path: string; is_project: boolean }[];
  shortcuts: { name: string; path: string }[];
}

export function ProjectBrowser({
  busy,
  error: openError,
  onOpen,
  onClose,
}: {
  busy: boolean;
  error: string;
  onOpen: (path: string) => Promise<void>;
  onClose: () => void;
}) {
  const [listing, setListing] = useState<FolderListing | null>(null);
  const [path, setPath] = useState("");
  const [search, setSearch] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const request = useRef(0);
  const browse = async (value: string) => {
    const token = ++request.current;
    setLoading(true);
    setError("");
    try {
      const result = await post<FolderListing>("/projects/browse", {
        path: value,
      });
      if (token !== request.current) return;
      setListing(result);
      setPath(result.path);
      setSearch("");
    } catch (err) {
      if (token === request.current)
        setError(err instanceof Error ? err.message : String(err));
    } finally {
      if (token === request.current) setLoading(false);
    }
  };
  useEffect(() => {
    void browse("");
    return () => {
      request.current++;
    };
  }, []);
  const folders =
    listing?.folders.filter((folder) =>
      folder.name.toLowerCase().includes(search.toLowerCase()),
    ) || [];
  return (
    <Dialog title="Browse for a project" onClose={onClose}>
      <p className="modal-description">
        Browse folders on the computer running MCAP Studio. Select a project
        folder containing project.json.
      </p>
      <div className="button-row browser-shortcuts">
        {listing?.shortcuts.map((item, index) => (
          <button
            key={`${item.path}-${index}`}
            className="button secondary small"
            disabled={busy || loading}
            onClick={() => void browse(item.path)}
          >
            <FolderOpen size={15} />
            {item.name}
          </button>
        ))}
      </div>
      <form
        className="inline-form"
        onSubmit={(event) => {
          event.preventDefault();
          void browse(path);
        }}
      >
        <input
          aria-label="Folder path"
          value={path}
          onChange={(event) => setPath(event.target.value)}
          placeholder="Enter a folder path"
        />
        <button
          className="button secondary small"
          disabled={loading || busy || !path.trim()}
        >
          Browse
        </button>
      </form>
      {(error || openError) && (
        <p className="inline-error" role="alert">
          {error || openError}
        </p>
      )}
      <div className="browser-toolbar">
        <button
          className="text-button"
          disabled={!listing?.parent || loading || busy}
          onClick={() => void browse(listing!.parent!)}
        >
          <ArrowLeft size={15} /> Up one folder
        </button>
        <input
          aria-label="Search folders"
          placeholder="Search this folder"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
        />
      </div>
      <div className="folder-list" aria-busy={loading}>
        {loading ? (
          <p>
            <LoaderCircle size={17} className="spin" /> Loading folders…
          </p>
        ) : (
          folders.map((folder) => (
            <button
              className="recording-row"
              key={folder.path}
              disabled={busy}
              onClick={() =>
                folder.is_project
                  ? void onOpen(folder.path)
                  : void browse(folder.path)
              }
            >
              <FolderOpen size={19} />
              <div>
                <strong>{folder.name}</strong>
                <span>
                  {folder.is_project ? "Load project" : "Browse folder"}
                </span>
              </div>
              <ArrowRight size={16} />
            </button>
          ))
        )}
        {!loading && !folders.length && (
          <p className="muted">
            No matching folders. Try another location or enter a project path
            below.
          </p>
        )}
      </div>
      <div className="modal-footer">
        <button className="button secondary" disabled={busy} onClick={onClose}>
          Cancel
        </button>
        <button
          className="button primary"
          disabled={busy || loading || !path.trim()}
          onClick={() => void onOpen(path.trim())}
        >
          {listing?.is_project && path === listing.path
            ? "Load this project"
            : "Open project path"}
        </button>
      </div>
    </Dialog>
  );
}
