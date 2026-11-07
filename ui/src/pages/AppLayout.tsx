import { createContext, createResource, createSignal, For, Show, useContext, type Resource } from "solid-js";
import { useLocation, useNavigate, useParams, type RouteSectionProps } from "@solidjs/router";
import { api, enc, ready, type ApiError, type Collection } from "../api";
import { Icon, toast } from "../ui";
import ErrorBox from "./ErrorBox";
import SchemaEditor from "./SchemaEditor";
import AppDashboard from "./AppDashboard";
import ProjectNav from "./ProjectNav";

interface AppCtx {
  collections: Resource<{ items: Collection[] }>;
  /** Opens the schema drawer; `null` starts a new collection. */
  editSchema: (c: Collection | null) => void;
}

const Ctx = createContext<AppCtx>();
export const useApp = () => useContext(Ctx)!;

/** One app: collections sidebar + routed content (PocketBase layout). */
export default function AppLayout(props: RouteSectionProps) {
  const params = useParams();
  const navigate = useNavigate();
  const [cols, { refetch }] = createResource(
    () => params.app,
    (app) => api<{ items: Collection[] }>("GET", `/api/v1/apps/${enc(app)}/collections`),
  );
  const [editing, setEditing] = createSignal<{ c: Collection | null } | null>(null);
  const appPath = () => `/apps/${enc(params.app!)}`;

  return (
    <Ctx.Provider value={{ collections: cols, editSchema: (c) => setEditing({ c }) }}>
      <div class="with-sidebar">
        <ProjectNav
          app={params.app!}
          collections={ready(cols)?.items ?? []}
          loading={cols.loading}
          activeColl={params.coll}
          onNewCollection={() => setEditing({ c: null })}
        />
        <section class="content">{props.children}</section>
      </div>
      <Show when={editing()}>
        {(e) => (
          <SchemaEditor
            app={params.app!}
            initial={e().c}
            collections={ready(cols)?.items ?? []}
            onClose={() => setEditing(null)}
            onSaved={async (c) => {
              await refetch();
              setEditing(null);
              toast(`Saved "${c.name}" (schema v${c.version})`);
              navigate(`${appPath()}/c/${enc(c.name)}`);
            }}
            onDeleted={async (name) => {
              await refetch();
              setEditing(null);
              toast(`Deleted "${name}"`);
              navigate(appPath());
            }}
          />
        )}
      </Show>
    </Ctx.Provider>
  );
}

export function AppHome() {
  const ctx = useApp();
  const params = useParams();
  return <AppDashboard app={params.app!} editSchema={() => ctx.editSchema(null)} />;
}
