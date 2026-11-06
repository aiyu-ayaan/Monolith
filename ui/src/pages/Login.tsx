import { createSignal } from "solid-js";
import { useNavigate } from "@solidjs/router";
import { api, setToken, type ApiError } from "../api";
import ErrorBox from "./ErrorBox";

export default function Login() {
  const [email, setEmail] = createSignal("");
  const [password, setPassword] = createSignal("");
  const [error, setError] = createSignal<ApiError | null>(null);
  const [busy, setBusy] = createSignal(false);
  const navigate = useNavigate();

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
    <div class="login">
      <form class="card" onSubmit={submit}>
        <h1>Monolith</h1>
        <label>
          Email
          <input type="email" autocomplete="username" required value={email()} onInput={(e) => setEmail(e.currentTarget.value)} />
        </label>
        <label>
          Password
          <input
            type="password"
            autocomplete="current-password"
            required
            value={password()}
            onInput={(e) => setPassword(e.currentTarget.value)}
          />
        </label>
        <ErrorBox error={error()} />
        <button type="submit" disabled={busy()}>
          {busy() ? "Signing in…" : "Sign in"}
        </button>
      </form>
    </div>
  );
}
