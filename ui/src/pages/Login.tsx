import { createResource, createSignal, Show } from "solid-js";
import { useNavigate } from "@solidjs/router";
import { api, setToken, type ApiError } from "../api";
import { AuthCard, Icon, PasswordInput } from "../ui";
import ErrorBox from "./ErrorBox";

export default function Login() {
  const [email, setEmail] = createSignal("");
  const [password, setPassword] = createSignal("");
  const [error, setError] = createSignal<ApiError | null>(null);
  const [busy, setBusy] = createSignal(false);
  const navigate = useNavigate();
  const [install] = createResource(() =>
    api<{ required: boolean }>("GET", "/api/v1/system/install").catch(() => ({ required: false })),
  );

  const submit = async (e: SubmitEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const r = await api<{ token: string }>("POST", "/api/v1/system/auth/login", { email: email(), password: password() });
      setToken(r.token);
      navigate("/", { replace: true });
    } catch (err) {
      setError(err as ApiError);
    } finally {
      setBusy(false);
    }
  };

  return (
    <AuthCard title="Sign in" subtitle="Superuser access to this Monolith server.">
      <Show when={install()?.required}>
        <div class="notice" role="note">
          <Icon name="alert" />
          <span>
            No superuser exists yet. Open the one-time setup link printed in the server console
            (<code>docker logs</code> in a container). Restart the server for a new link.
          </span>
        </div>
      </Show>
      <form class="stack" onSubmit={submit}>
        <div class="field">
          <label for="login-email">Email</label>
          <input id="login-email" type="email" autocomplete="username" required value={email()} onInput={(e) => setEmail(e.currentTarget.value)} />
        </div>
        <div class="field">
          <label for="login-password">Password</label>
          <PasswordInput id="login-password" autocomplete="current-password" value={password()} onInput={setPassword} />
        </div>
        <ErrorBox error={error()} />
        <button class="btn block" type="submit" disabled={busy()}>
          {busy() ? "Signing in…" : "Sign in"}
        </button>
      </form>
    </AuthCard>
  );
}
