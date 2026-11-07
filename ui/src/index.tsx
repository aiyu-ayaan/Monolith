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
import Auth from "./pages/Auth";
import Logs from "./pages/Logs";
import Rules from "./pages/Rules";
import Realtime from "./pages/Realtime";
import Backups from "./pages/Backups";
import "./styles.css";

function Shell(props: RouteSectionProps) {
  const loc = useLocation();
  const isAnalytics = () => loc.pathname === "/analytics";
  // Inside a project the console shows only that project's navigation (Firebase-style).
  const inApp = () => loc.pathname.startsWith("/apps/");
  return (
    <Show when={token()} fallback={<Navigate href="/login" />}>
      <div class="shell" classList={{ "no-rail": inApp() }}>
        <Show when={!inApp()}>
        <nav class="rail" aria-label="Main">
          <a href="#/" class="logo" aria-label="Monolith home">
            <Icon name="logo" size={22} />
          </a>
          <a href="#/" class="rail-btn" classList={{ active: !isAnalytics() }} aria-label="Dashboard" title="Dashboard">
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
        </Show>
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
            <Route path="/auth" component={Auth} />
            <Route path="/logs" component={Logs} />
            <Route path="/rules" component={Rules} />
            <Route path="/realtime" component={Realtime} />
            <Route path="/backups" component={Backups} />
            <Route path="/c/:coll" component={Records} />
          </Route>
        </Route>
      </HashRouter>
      <Toasts />
    </>
  ),
  document.getElementById("root")!,
);
