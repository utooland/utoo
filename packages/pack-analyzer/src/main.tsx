import { ThemeProvider } from "next-themes";
import {
  Component,
  type ReactNode,
  useCallback,
  useDeferredValue,
  useEffect,
  useMemo,
  useState,
} from "react";
import { createRoot } from "react-dom/client";
import useSWR from "swr";
import { FileSearch } from "@/components/file-search";
import { Sidebar } from "@/components/sidebar";
import { TreemapVisualizer } from "@/components/treemap-visualizer";
import { Button } from "@/components/ui/button";
import { MultiSelect } from "@/components/ui/multi-select";
import {
  type AnalyzeData,
  assertMatchingModuleIndex,
  type ModulesData,
} from "@/lib/analyze-data";
import {
  analyzeDataUrl,
  fetchAnalyzeData,
  fetchModulesData,
} from "@/lib/analyzer-data";
import {
  computeModuleDepthMap,
  computeSourceLoadScopes,
} from "@/lib/module-graph";
import {
  createAnalyzeTreemapSource,
  type LayoutNodeInfo,
} from "@/lib/treemap-layout";
import { useSidebarResize } from "@/lib/use-sidebar-resize";
import { formatBytes, jsonFetcher } from "@/lib/utils";
import { type Entry, EntryPicker } from "./entry-picker";

const dataPath = "./data";
const staticDataOptions = {
  revalidateOnFocus: false,
  revalidateOnReconnect: false,
  shouldRetryOnError: false,
};

const fileTypes = [
  { value: "js", label: "JavaScript" },
  { value: "css", label: "CSS" },
  { value: "json", label: "JSON" },
  { value: "asset", label: "Assets" },
];

// This method is added to the upstream parser by build.mjs. It reads the
// analyzer header that the native writer emitted for the selected Pack entry.
interface PackAnalyzeData extends AnalyzeData {
  packEntryModuleIdents(): string[];
}

function ReportStatus({ children }: { children: ReactNode }) {
  return (
    <div className="grid flex-1 place-items-center p-6 text-center text-sm text-muted-foreground">
      {children}
    </div>
  );
}

function ReportError({ error }: { error: Error }) {
  return (
    <ReportStatus>
      <div role="alert">
        <p className="font-semibold text-foreground">
          Could not load analyzer report
        </p>
        <p className="mt-1">{error.message}</p>
      </div>
    </ReportStatus>
  );
}

class ReportErrorBoundary extends Component<
  { children: ReactNode },
  { error: Error | null }
> {
  state: { error: Error | null } = { error: null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  render() {
    return this.state.error ? (
      <ReportError error={this.state.error} />
    ) : (
      this.props.children
    );
  }
}

async function fetchEntries(url: string): Promise<Entry[]> {
  const entries = await jsonFetcher<unknown>(url);
  if (
    !Array.isArray(entries) ||
    entries.some(
      (entry) =>
        typeof entry?.name !== "string" ||
        typeof entry?.path !== "string" ||
        !/^\/entry-\d+$/.test(entry.path) ||
        (entry.environment !== undefined &&
          entry.environment !== "client" &&
          entry.environment !== "server"),
    )
  ) {
    throw new Error("Invalid entry index.");
  }
  // Reports created before server support contain browser entries only.
  return entries.map((entry) => ({
    ...entry,
    environment: entry.environment ?? "client",
  }));
}

function Analyzer() {
  const { data: entries, error: indexError } = useSWR(
    `${dataPath}/entries.json`,
    fetchEntries,
    staticDataOptions,
  );
  const { data: modulesData, error: modulesError } = useSWR(
    `${dataPath}/modules.data`,
    fetchModulesData,
    staticDataOptions,
  );
  const [selection, setSelection] = useState(() => {
    const params = new URLSearchParams(location.search);
    const environment = params.get("environment");
    return {
      path: params.get("entry"),
      environment:
        environment === "client" || environment === "server"
          ? environment
          : undefined,
    };
  });
  const [search, setSearch] = useState("");
  const [typeFilter, setTypeFilter] = useState(() =>
    fileTypes.map(({ value }) => value),
  );
  const [initialOnly, setInitialOnly] = useState(false);
  const requestedEntry = entries?.find(({ path }) => path === selection.path);
  const requestedEnvironment =
    selection.environment ?? requestedEntry?.environment;
  const environment = entries?.some(
    (entry) => entry.environment === requestedEnvironment,
  )
    ? requestedEnvironment
    : entries?.[0]?.environment;
  const visibleEntries = entries?.filter(
    (entry) => entry.environment === environment,
  );
  const selectedEntry =
    visibleEntries?.find(({ path }) => path === selection.path) ??
    visibleEntries?.[0];
  const deferredSearch = useDeferredValue(search);

  const selectEntry = (entry: Entry) => {
    setSelection({ path: entry.path, environment: entry.environment });
    const url = new URL(location.href);
    url.searchParams.set("entry", entry.path);
    url.searchParams.set("environment", entry.environment);
    history.replaceState(null, "", url);
  };

  return (
    <main className="flex h-dvh flex-col bg-background text-foreground">
      <header className="flex flex-wrap items-center gap-3 border-b bg-background p-3">
        <h1 className="text-sm font-semibold">Utoopack Bundle Analyzer</h1>
        {selectedEntry && entries && visibleEntries ? (
          <>
            <div
              role="group"
              aria-label="Bundle environment"
              className="flex gap-1"
            >
              {(["client", "server"] as const).map((value) => (
                <Button
                  key={value}
                  variant={environment === value ? "secondary" : "outline"}
                  size="sm"
                  aria-pressed={environment === value}
                  disabled={
                    !entries.some((entry) => entry.environment === value)
                  }
                  onClick={() => {
                    const entry = entries.find(
                      (entry) => entry.environment === value,
                    );
                    if (entry) selectEntry(entry);
                  }}
                >
                  {value === "client" ? "Client" : "Server"}
                </Button>
              ))}
            </div>
            <EntryPicker
              entries={visibleEntries}
              selectedEntry={selectedEntry}
              onSelect={selectEntry}
            />
            <FileSearch value={search} onChange={setSearch} />
            <MultiSelect
              options={fileTypes}
              value={typeFilter}
              onValueChange={setTypeFilter}
              selectionName={{ singular: "file type", plural: "file types" }}
              aria-label="Filter file types"
            />
            <label
              className="flex items-center gap-2 text-xs"
              title="Keep modules reachable without crossing an async import boundary"
            >
              <input
                type="checkbox"
                checked={initialOnly}
                onChange={(event) => setInitialOnly(event.target.checked)}
              />
              Initial modules only
            </label>
          </>
        ) : null}
      </header>
      {indexError || modulesError ? (
        <ReportError error={indexError ?? modulesError} />
      ) : selectedEntry && modulesData ? (
        <ReportErrorBoundary key={selectedEntry.path}>
          <EntryReport
            entry={selectedEntry}
            modulesData={modulesData}
            search={deferredSearch}
            typeFilter={typeFilter}
            initialOnly={initialOnly}
          />
        </ReportErrorBoundary>
      ) : (
        <ReportStatus>
          <p role="status">
            {entries?.length === 0
              ? "No entries to analyze."
              : "Loading analyzer report…"}
          </p>
        </ReportStatus>
      )}
    </main>
  );
}

interface EntryReportProps {
  entry: Entry;
  modulesData: ModulesData;
  search: string;
  typeFilter: string[];
  initialOnly: boolean;
}

function EntryReport(props: EntryReportProps) {
  const { data: analyzeData, error } = useSWR(
    analyzeDataUrl(dataPath, props.entry.path),
    fetchAnalyzeData,
    staticDataOptions,
  );
  if (error) return <ReportError error={error} />;
  if (!analyzeData) {
    return (
      <ReportStatus>
        <p role="status">Loading {props.entry.name}…</p>
      </ReportStatus>
    );
  }
  return (
    <EntryTreemap {...props} analyzeData={analyzeData as PackAnalyzeData} />
  );
}

function EntryTreemap({
  entry,
  analyzeData,
  modulesData,
  search,
  typeFilter,
  initialOnly,
}: EntryReportProps & { analyzeData: PackAnalyzeData }) {
  const source = useMemo(
    () => createAnalyzeTreemapSource(analyzeData),
    [analyzeData],
  );
  const [selected, setSelected] = useState(source.rootIndex);
  const [focused, setFocused] = useState(source.rootIndex);
  const [hovered, setHovered] = useState<LayoutNodeInfo | null>(null);
  const [mouseInside, setMouseInside] = useState(false);
  const { sidebarWidth, startResizing } = useSidebarResize(25);

  const { moduleDepthMap, sourceLoadScopes } = useMemo(() => {
    assertMatchingModuleIndex(modulesData, analyzeData, "Current report");
    const rootIdents = new Set(analyzeData.packEntryModuleIdents());
    const rootIndices: number[] = [];
    for (let index = 0; index < modulesData.moduleCount(); index++) {
      const module = modulesData.module(index);
      if (module && rootIdents.has(module.ident)) rootIndices.push(index);
    }

    return {
      moduleDepthMap: computeModuleDepthMap(modulesData, rootIndices),
      sourceLoadScopes: computeSourceLoadScopes(
        modulesData,
        analyzeData,
        rootIndices,
      ),
    };
  }, [analyzeData, modulesData]);

  const filterSource = useCallback(
    (sourceIndex: number) => {
      const flags = analyzeData.getSourceFlags(sourceIndex);
      const scope = sourceLoadScopes.get(sourceIndex);
      return (
        flags[entry.environment] &&
        typeFilter.some(
          (type) => flags[type as "js" | "css" | "json" | "asset"],
        ) &&
        (!initialOnly || scope === "initial" || scope === "mixed")
      );
    },
    [analyzeData, entry.environment, initialOnly, sourceLoadScopes, typeFilter],
  );
  const getFileLoadScope = useCallback(
    (sourceIndex: number) => sourceLoadScopes.get(sourceIndex) ?? "unknown",
    [sourceLoadScopes],
  );

  useEffect(() => {
    const resetSelection = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !event.defaultPrevented) {
        setSelected(source.rootIndex);
        setFocused(source.rootIndex);
      }
    };
    window.addEventListener("keydown", resetSelection);
    return () => window.removeEventListener("keydown", resetSelection);
  }, [source.rootIndex]);

  if (analyzeData.sourceCount() === 0) {
    return (
      <ReportStatus>
        <p role="status">
          This entry has no {entry.environment} bundle modules.
        </p>
      </ReportStatus>
    );
  }

  return (
    <>
      <div className="flex min-h-0 flex-1">
        <div
          className="min-w-0 flex-1"
          onMouseEnter={() => setMouseInside(true)}
          onMouseLeave={() => setMouseInside(false)}
        >
          <TreemapVisualizer
            source={source}
            selectedSourceIndex={selected}
            onSelectSourceIndex={setSelected}
            focusedSourceIndex={focused}
            onFocusSourceIndex={setFocused}
            isMouseInTreemap={mouseInside}
            onHoveredNodeChange={setHovered}
            searchQuery={search}
            filterSource={filterSource}
            getFileLoadScope={getFileLoadScope}
            overlay={<AsyncScopeLegend />}
          />
        </div>
        <button
          type="button"
          className="w-1 flex-none cursor-col-resize bg-border transition-colors hover:bg-primary"
          onMouseDown={startResizing}
          aria-label="Resize sidebar"
        />
        <Sidebar
          sidebarWidth={sidebarWidth}
          analyzeData={analyzeData}
          modulesData={modulesData}
          selectedSourceIndex={selected}
          moduleDepthMap={moduleDepthMap}
          environmentFilter={entry.environment}
          filterSource={filterSource}
        />
      </div>
      <footer className="h-10 flex-none truncate border-t px-4 py-2 text-sm text-muted-foreground">
        {hovered ? (
          <>
            <span className="font-medium text-foreground">{hovered.name}</span>
            {" · "}
            {formatBytes(hovered.size)} estimated compressed
          </>
        ) : (
          "Hover over a file to see details"
        )}
      </footer>
    </>
  );
}

// The tile patterns are drawn by the upstream visualizer.
function AsyncScopeLegend() {
  return (
    <div
      className="absolute bottom-3 left-3 flex flex-wrap gap-3 rounded border bg-background/95 px-2.5 py-1.5 text-xs shadow-sm"
      title="Reachability in the import graph, independent of browser requests"
    >
      <span className="flex items-center gap-1.5">
        <span className="h-3 w-3 border border-foreground/40 bg-[repeating-linear-gradient(135deg,transparent_0,transparent_3px,currentColor_3px,currentColor_4px)] text-foreground/45" />
        Behind async boundary
      </span>
      <span className="flex items-center gap-1.5">
        <span className="relative h-3 w-3 border border-foreground/40 after:absolute after:right-0 after:top-0 after:h-0 after:w-0 after:border-l-[5px] after:border-t-[5px] after:border-l-transparent after:border-t-foreground/60" />
        Initial + async paths
      </span>
    </div>
  );
}

const root = document.getElementById("root");
if (!root) throw new Error("The analyzer page is missing its root element.");

createRoot(root).render(
  <ThemeProvider attribute="class" defaultTheme="system" enableSystem>
    <ReportErrorBoundary>
      <Analyzer />
    </ReportErrorBoundary>
  </ThemeProvider>,
);
