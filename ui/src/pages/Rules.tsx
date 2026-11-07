import { For, Show } from "solid-js";
import { useParams } from "@solidjs/router";
import { enc, ready, RULE_KEYS, type Collection } from "../api";
import { Icon } from "../ui";
import ErrorBox from "./ErrorBox";
import { useApp } from "./AppLayout";

const Cell = (p: { rule: string | null | undefined }) => (
  <Show when={p.rule !== null && p.rule !== undefined} fallback={<span class="badge" title="Only superusers">Locked</span>}>
    <Show when={p.rule} fallback={<span class="badge on" title="Anyone, including anonymous callers">Public</span>}>
      <code class="rule-expr" title={p.rule!}>{p.rule}</code>
    </Show>
  </Show>
);

/** Every collection's five rules side by side; edit opens the schema drawer on its rules tab. */
export default function Rules() {
  const params = useParams();
  const ctx = useApp();
  const items = () => ready(ctx.collections)?.items ?? [];
  return (
    <div class="page wide">
      <header class="page-head">
        <div class="min0">
          <p class="crumbs">
            <a href={`#/apps/${enc(params.app!)}`}>{params.app}</a>
          </p>
          <h1>Access rules</h1>
          <p class="muted">Who can list, view, create, update and delete records. Locked = superusers only, Public = anyone.</p>
        </div>
      </header>
      <ErrorBox error={ctx.collections.error} />
      <div class="table-wrap">
        <table class="grid">
          <thead>
            <tr>
              <th scope="col">Collection</th>
              <For each={RULE_KEYS}>{(k) => <th scope="col">{k}</th>}</For>
              <th scope="col"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            <For each={items()} fallback={<tr><td colspan="7" class="empty-row">{ctx.collections.loading ? "Loading…" : "No collections yet."}</td></tr>}>
              {(c: Collection) => (
                <tr>
                  <td>
                    <a class="mono link" href={`#/apps/${enc(params.app!)}/c/${enc(c.name)}`}>{c.name}</a>
                  </td>
                  <For each={RULE_KEYS}>
                    {(k) => (
                      <td>
                        <Cell rule={c.rules?.[k]} />
                      </td>
                    )}
                  </For>
                  <td class="num">
                    <button class="btn secondary sm" onClick={() => ctx.editSchema(c, "rules")}>
                      <Icon name="shield" size={14} />
                      Edit
                    </button>
                  </td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </div>
    </div>
  );
}
