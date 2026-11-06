import { createSignal, Show } from "solid-js";
import { useNavigate, useParams } from "@solidjs/router";
import { api, setToken, type ApiError } from "../api";
import { AuthCard, PasswordInput, toast } from "../ui";
import ErrorBox from "./ErrorBox";

/** First-superuser setup, reached through the one-time link the server prints (doc 18 §2). */
export default function Install() {
  const params = useParams();
  const navigate = useNavigate();
  const [email, setEmail] = createSignal("");
  const [password, setPassword] = createSignal("");
  const [confirm, setConfirm] = createSignal("");
  const [mismatch, setMismatch] = createSignal(false);
  const [error, setError] = createSignal<ApiError | null>(null);
  const [busy, setBusy] = createSignal(false);

  const submit = async (e: SubmitEvent) => {
    e.preventDefault();
    setError(null);
    setMismatch(password() !== confirm());
    if (mismatch()) return;
    setBusy(true);
    try {
      const r = await api<{ token: string }>("POST", "/api/v1/system/install", {
        token: params.token,
        email: email(),
        password: password(),
      });
      setToken(r.token);
      toast("Superuser created");
      navigate("/", { replace: true });
    } catch (err) {
      setError(err as ApiError);
    } finally {
      setBusy(false);
    }
  };

  return (
    <AuthCard title="Create the first superuser" subtitle="This one-time link works once and expires an hour after the server started.">
      <form class="stack" onSubmit={submit}>
        <div class="field">
          <label for="install-email">Email</label>
          <input id="install-email" type="email" autocomplete="username" required value={email()} onInput={(e) => setEmail(e.currentTarget.value)} />
        </div>
        <div class="field">
          <label for="install-password">Password</label>
          <PasswordInput id="install-password" autocomplete="new-password" minLength={10} describedBy="install-password-help" value={password()} onInput={setPassword} />
          <p id="install-password-help" class="help">At least 10 characters. Stored as an Argon2id hash.</p>
        </div>
        <div class="field">
          <label for="install-confirm">Confirm password</label>
          <PasswordInput
            id="install-confirm"
            autocomplete="new-password"
            minLength={10}
            describedBy={mismatch() ? "install-confirm-error" : undefined}
            value={confirm()}
            onInput={setConfirm}
          />
          <Show when={mismatch()}>
            <p id="install-confirm-error" class="field-error" role="alert">
              Passwords do not match.
            </p>
          </Show>
        </div>
        <Show
          when={error()?.problem.status === 401}
          fallback={<ErrorBox error={error()} />}
        >
          <div class="error" role="alert">
            <strong>This setup link is invalid, expired or already used.</strong> Restart the server to print a new one, or
            <a href="#/login"> sign in</a> if a superuser exists.
          </div>
        </Show>
        <button class="btn block" type="submit" disabled={busy()}>
          {busy() ? "Creating…" : "Create superuser"}
        </button>
      </form>
    </AuthCard>
  );
}
