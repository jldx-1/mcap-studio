

export async function api<T>(
  path: string,
  options: RequestInit = {},
): Promise<T> {
  const response = await fetch(`/api${path}`, {
    credentials: "same-origin",
    ...options,
    headers: {
      "X-Requested-With": "mcap-workbench",
      ...(options.body && !(options.body instanceof FormData)
        ? { "Content-Type": "application/json" }
        : {}),
      ...options.headers,
    },
  });
  if (!response.ok) {
    if (response.status === 401 && !path.startsWith("/auth/")) window.dispatchEvent(new Event("mcap-session-expired"));
    const value = await response.text();
    const tooLarge = "File is too large. It exceeds the server's upload size limit. Choose a smaller file.";
    let message = response.status === 413 ? tooLarge : value;
    try {
      const error = JSON.parse(value);
      const detail = error.error || error.message;
      if (typeof detail === "string" && detail.trim()) {
        message = response.status === 413 && !/too large/i.test(detail) ? `${tooLarge} ${detail}` : detail;
      }
    } catch {
      /* plain server error */
    }
    throw new Error(message || `Request failed (${response.status}).`);
  }
  return response.status === 204 ? (undefined as T) : response.json();
}

export const post = <T>(path: string, value: unknown) =>
  api<T>(path, { method: "POST", body: JSON.stringify(value) });

export const put = <T>(path: string, value: unknown) =>
  api<T>(path, { method: "PUT", body: JSON.stringify(value) });

export const patch = <T>(path: string, value: unknown) =>
  api<T>(path, { method: "PATCH", body: JSON.stringify(value) });
