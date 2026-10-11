import { useState, type FormEvent } from "react";
import type { Session } from "../types/accounts";

export function SignInPage({ auth }: { auth: { session: Session | null; error: string; refresh: () => Promise<void>; signIn: (value: { username: string; password: string; name?: string }) => Promise<void> } }) {
  const [username, setUsername] = useState("");
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const setup = auth.session?.setup_required;
  const submit = async (event: FormEvent) => {
    event.preventDefault(); setBusy(true);setError("");
    try { await auth.signIn({ username, password, ...(setup ? { name } : {}) });setPassword(""); }
    catch (err) { setError(err instanceof Error ? err.message : String(err)); }
    finally { setBusy(false); }
  };
  return <main className="auth-page"><section className="panel auth-panel"><div className="panel-heading"><div><h1>{setup ? "Set up your workspace" : "Sign in to MCAP Studio"}</h1><p>{setup ? "Create the first administrator account. Then add users, groups, and project members." : "Use the account your workspace administrator created for you."}</p></div></div>
    {(error || auth.error) && <p className="inline-error" role="alert">{error || auth.error}</p>}
    {!auth.session ? <button className="button secondary" onClick={() => void auth.refresh()}>Reconnect</button> : <form className="account-form" onSubmit={(event) => void submit(event)}>
      {setup && <label>Your name<input required value={name} onChange={(e) => setName(e.target.value)} autoComplete="name" /></label>}
      <label>Username<input required value={username} onChange={(e) => setUsername(e.target.value)} autoComplete="username" maxLength={120} /></label>
      <label>Password<input type="password" required minLength={setup ? 12 : undefined} maxLength={256} value={password} onChange={(e) => setPassword(e.target.value)} autoComplete={setup ? "new-password" : "current-password"} /></label>
      {setup && <p className="field-help">Use at least 12 characters. No default password is created.</p>}
      <button className="button primary" type="submit" disabled={busy}>{busy ? "Signing in…" : setup ? "Create administrator" : "Sign in"}</button>
    </form>}
  </section></main>;
}
