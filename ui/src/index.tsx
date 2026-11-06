import { render } from "solid-js/web";
import { HashRouter, Navigate, Route, useLocation, type RouteSectionProps } from "@solidjs/router";
import { Show } from "solid-js";
import { setToken, token } from "./api";
import { Icon, Toasts } from "./ui";
import Login from "./pages/Login";
import Install from "./pages/Install";
import Apps from "./pages/Apps";
import AppLayout, { AppHome } from "./pages/AppLayout";
import { AppAnalytics, SystemAnalytics } from "./pages/Analytics";
import Records from "./pages/Records";
import "./styles.css";

function Shell(props: RouteSectionProps) {
  const loc = useLocation();
  const isAnalytics = () => loc.pathname === "/analytics";
  return (
    <Show when={token()} fallback={<Navigate href="/login" />}>
      <div class="shell">
        <nav class="rail" aria-label="Main">
          <a href="#/" class="logo" aria-label="Monolith home">
            <Icon name="logo" size={22} />
          </a>
          <a href="#/" class="rail-btn" classList={{ active: !isAnalytics() }} aria-label="Apps" title="Apps">
            <Icon name="layers" />
          </a>
          <a href="#/analytics" class="rail-btn" classList={{ active: isAnalytics() }} aria-label="Analytics" title="Analytics">
            <Icon name="chart" />
          </a>
          <span class="grow" />
          <button class="rail-btn" aria-label="Sign out" title="Sign out" onClick={() => setToken(null)}>
            <Icon name="logout" />
          </button>
        </nav>
        <main class="shell-main">{props.children}</main>
      </div>
    </Show>
  );
}

render(
  () => (
    <>
      <HashRouter>
        <Route path="/login" component={Login} />
        <Route path="/install/:token" component={Install} />
        <Route path="/" component={Shell}>
          <Route path="/" component={Apps} />
          <Route path="/analytics" component={SystemAnalytics} />
          <Route path="/apps/:app" component={AppLayout}>
            <Route path="/" component={AppHome} />
            <Route path="/analytics" component={AppAnalytics} />
            <Route path="/c/:coll" component={Records} />
          </Route>
        </Route>
      </HashRouter>
      <Toasts />
    </>
  ),
  document.getElementById("root")!,
);
