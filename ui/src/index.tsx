import { render } from "solid-js/web";
import { HashRouter, Navigate, Route, type RouteSectionProps } from "@solidjs/router";
import { Show } from "solid-js";
import { setToken, token } from "./api";
import Login from "./pages/Login";
import Apps from "./pages/Apps";
import Collections from "./pages/Collections";
import Records from "./pages/Records";
import "./styles.css";

function Shell(props: RouteSectionProps) {
  return (
    <Show when={token()} fallback={<Navigate href="/login" />}>
      <header class="top">
        <a href="#/" class="brand">
          Monolith
        </a>
        <button class="ghost" onClick={() => setToken(null)}>
          Sign out
        </button>
      </header>
      <main>{props.children}</main>
    </Show>
  );
}

render(
  () => (
    <HashRouter>
      <Route path="/login" component={Login} />
      <Route path="/" component={Shell}>
        <Route path="/" component={Apps} />
        <Route path="/apps/:app" component={Collections} />
        <Route path="/apps/:app/c/:coll" component={Records} />
      </Route>
    </HashRouter>
  ),
  document.getElementById("root")!,
);
