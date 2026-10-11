import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { api, post } from "../services/http";
import type { Session } from "../types/accounts";
import { SignInPage } from "../pages/SignInPage";

function useSession() {
  const [session, setSession] = useState<Session | null>(null);
  const [error, setError] = useState("");
  const refresh = useCallback(async () => {
    try { setSession(await api<Session>("/auth/session")); setError(""); }
    catch (err) { setError(String(err)); }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);
  useEffect(() => {
    const check = () => { if(document.visibilityState === "visible") void refresh(); };
    const timer = window.setInterval(check,60000);
    window.addEventListener("focus",check);
    return () => { window.clearInterval(timer);window.removeEventListener("focus",check); };
  },[refresh]);
  useEffect(() => {
    const expired = () => { setSession((s) => s ? { ...s, user: null } : s); setError("Your session ended. Sign in again to continue."); };
    window.addEventListener("mcap-session-expired", expired);
    return () => window.removeEventListener("mcap-session-expired", expired);
  }, []);
  const signIn = async (request: { username: string; password: string; name?: string }) => {
    await post(`/auth/${session?.setup_required ? "setup" : "login"}`, request);
    await refresh();
  };
  const signOut = async () => { await post("/auth/logout", {}); await refresh(); };
  return { session, error, refresh, signIn, signOut, user: session?.user || null };
}
type Auth = ReturnType<typeof useSession>;
const AuthContext = createContext<Auth | null>(null);
export function AuthProvider({ children }: { children: ReactNode }) {
  const auth = useSession();
  return <AuthContext.Provider value={auth}>{auth.user ? children : <SignInPage auth={auth} />}</AuthContext.Provider>;
}
export function useAuth() {
  const auth = useContext(AuthContext);
  if (!auth) throw new Error("AuthProvider is required");
  return auth;
}
