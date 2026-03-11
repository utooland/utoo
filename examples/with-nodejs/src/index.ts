import { serve } from "@hono/node-server";
import dayjs from "dayjs";
import duration from "dayjs/plugin/duration";
import relativeTime from "dayjs/plugin/relativeTime";
import { Hono } from "hono";
import { logger } from "hono/logger";
import { prettyJSON } from "hono/pretty-json";
import { endTime, startTime, timing } from "hono/timing";
import { capitalize, groupBy, mean, sortBy, sumBy } from "lodash-es";

dayjs.extend(relativeTime);
dayjs.extend(duration);

// ── Types ──────────────────────────────────────────────────────
interface Task {
  id: number;
  title: string;
  assignee: string;
  priority: "low" | "medium" | "high" | "critical";
  estimatedHours: number;
  createdAt: string;
  done: boolean;
}

// ── Sample dataset ─────────────────────────────────────────────
const tasks: Task[] = [
  {
    id: 1,
    title: "Setup CI pipeline",
    assignee: "alice",
    priority: "high",
    estimatedHours: 8,
    createdAt: "2026-03-01T09:00:00Z",
    done: true,
  },
  {
    id: 2,
    title: "Write unit tests",
    assignee: "bob",
    priority: "medium",
    estimatedHours: 5,
    createdAt: "2026-03-02T10:30:00Z",
    done: false,
  },
  {
    id: 3,
    title: "Fix login bug",
    assignee: "alice",
    priority: "critical",
    estimatedHours: 3,
    createdAt: "2026-03-03T14:15:00Z",
    done: true,
  },
  {
    id: 4,
    title: "Update docs",
    assignee: "charlie",
    priority: "low",
    estimatedHours: 2,
    createdAt: "2026-03-04T08:00:00Z",
    done: false,
  },
  {
    id: 5,
    title: "Refactor API layer",
    assignee: "bob",
    priority: "high",
    estimatedHours: 12,
    createdAt: "2026-03-05T11:00:00Z",
    done: false,
  },
  {
    id: 6,
    title: "Database migration",
    assignee: "alice",
    priority: "critical",
    estimatedHours: 6,
    createdAt: "2026-03-06T16:00:00Z",
    done: false,
  },
  {
    id: 7,
    title: "Add dark mode",
    assignee: "charlie",
    priority: "medium",
    estimatedHours: 4,
    createdAt: "2026-03-07T09:30:00Z",
    done: true,
  },
  {
    id: 8,
    title: "Performance audit",
    assignee: "bob",
    priority: "high",
    estimatedHours: 7,
    createdAt: "2026-03-08T13:00:00Z",
    done: false,
  },
];

const priorityOrder: Record<string, number> = {
  critical: 0,
  high: 1,
  medium: 2,
  low: 3,
};

// ── Hono app ───────────────────────────────────────────────────
const app = new Hono();

// Middleware
app.use("*", logger());
app.use("*", timing());
app.use("*", prettyJSON());

// GET / — server info
app.get("/", (c) => {
  return c.json({
    name: "utoopack with-nodejs example",
    version: process.env.APP_VERSION ?? "dev",
    runtime: `Node ${process.version}`,
    platform: `${process.platform}-${process.arch}`,
    uptime: `${Math.floor(process.uptime())}s`,
    timestamp: dayjs().format("YYYY-MM-DD HH:mm:ss"),
  });
});

// GET /tasks — list all tasks
app.get("/tasks", (c) => {
  startTime(c, "db");
  const filter = c.req.query("status");
  const assignee = c.req.query("assignee");

  let result = [...tasks];
  if (filter === "done") result = result.filter((t) => t.done);
  if (filter === "pending") result = result.filter((t) => !t.done);
  if (assignee) result = result.filter((t) => t.assignee === assignee);

  endTime(c, "db");
  return c.json({ count: result.length, tasks: result });
});

// GET /tasks/:id — single task
app.get("/tasks/:id", (c) => {
  const id = Number(c.req.param("id"));
  const task = tasks.find((t) => t.id === id);
  if (!task) return c.json({ error: "Task not found" }, 404);

  const created = dayjs(task.createdAt);
  return c.json({
    ...task,
    formattedDate: created.format("ddd, MMM D YYYY HH:mm"),
    timeAgo: created.fromNow(),
  });
});

// GET /stats — project statistics (lodash-es + dayjs)
app.get("/stats", (c) => {
  startTime(c, "analysis");

  // Group by assignee
  const byAssignee = groupBy(tasks, "assignee");
  const assigneeStats = Object.entries(byAssignee).map(([person, items]) => ({
    assignee: capitalize(person),
    totalTasks: items.length,
    completed: items.filter((t) => t.done).length,
    estimatedHours: sumBy(items, "estimatedHours"),
  }));

  // Sort by priority
  const byPriority = sortBy(tasks, (t) => priorityOrder[t.priority]);
  const prioritySummary = Object.entries(groupBy(byPriority, "priority")).map(
    ([priority, items]) => ({
      priority,
      count: items.length,
      hours: sumBy(items, "estimatedHours"),
    }),
  );

  // Timeline
  const firstTask = dayjs(tasks[0].createdAt);
  const lastTask = dayjs(tasks[tasks.length - 1].createdAt);
  const span = dayjs.duration(lastTask.diff(firstTask));

  endTime(c, "analysis");

  return c.json({
    overview: {
      totalTasks: tasks.length,
      completed: tasks.filter((t) => t.done).length,
      pending: tasks.filter((t) => !t.done).length,
      totalHours: sumBy(tasks, "estimatedHours"),
      avgHoursPerTask: Number(
        mean(tasks.map((t) => t.estimatedHours)).toFixed(1),
      ),
      completionRate: `${((tasks.filter((t) => t.done).length / tasks.length) * 100).toFixed(0)}%`,
    },
    assignees: assigneeStats,
    priorities: prioritySummary,
    timeline: {
      start: firstTask.format("YYYY-MM-DD"),
      end: lastTask.format("YYYY-MM-DD"),
      spanDays: span.days(),
      spanHours: span.hours(),
    },
  });
});

// GET /health — health check
app.get("/health", (c) => {
  return c.json({
    status: "ok",
    pid: process.pid,
    memory: process.memoryUsage(),
  });
});

// 404 fallback
app.notFound((c) => {
  return c.json(
    {
      error: "Not Found",
      routes: [
        "GET /",
        "GET /tasks",
        "GET /tasks/:id",
        "GET /stats",
        "GET /health",
      ],
    },
    404,
  );
});

// ── Start server ───────────────────────────────────────────────
const port = Number(process.env.PORT) || 3456;

serve({ fetch: app.fetch, port }, (info) => {
  console.log(`
╔════════════════════════════════════════════╗
║   utoopack Node.js Example (Hono server)  ║
╚════════════════════════════════════════════╝

  🚀 Server running at http://localhost:${info.port}

  Routes:
    GET /           → Server info
    GET /tasks      → List tasks (?status=done|pending&assignee=alice)
    GET /tasks/:id  → Get task by ID
    GET /stats      → Project statistics
    GET /health     → Health check

  Bundled with utoopack | Node ${process.version} | ${process.platform}-${process.arch}
`);
});
