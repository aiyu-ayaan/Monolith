import { For, Show } from "solid-js";
import type { ApiError } from "../api";

/** Shows a problem+json error with its field errors and request id (doc 22). */
export default function ErrorBox(props: { error: ApiError | null | undefined }) {
  return (
    <Show when={props.error}>
      {(e) => (
        <div class="error" role="alert">
          <strong>{e().problem.title}</strong>
          <Show when={e().problem.detail}>: {e().problem.detail}</Show>
          <Show when={e().problem.errors?.length}>
            <ul>
              <For each={e().problem.errors}>
                {(f) => (
                  <li>
                    <code>{f.field}</code> {f.message}
                  </li>
                )}
              </For>
            </ul>
          </Show>
          <Show when={e().problem.requestId}>
            <div class="muted">request id {e().problem.requestId}</div>
          </Show>
        </div>
      )}
    </Show>
  );
}
