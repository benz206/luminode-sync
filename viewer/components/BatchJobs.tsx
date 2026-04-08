"use client";

import { useCallback, useEffect, useRef, useState } from "react";

// ── Types ─────────────────────────────────────────────────────────────────────

interface JobInfo {
  job_id: string;
  spotify_id: string;
  title: string | null;
  status: "pending" | "running" | "done" | "failed" | "cancelled";
  error?: string | null;
}

interface BatchStatus {
  batch_id: string;
  total: number;
  pending: number;
  running: number;
  done: number;
  failed: number;
  jobs: JobInfo[];
}

interface StoredBatch {
  batch_id: string;
  submitted_at: number;
  url: string;
  total: number;
}

type ViewMode = "batches" | "alljobs";
type StatusFilter = "all" | JobInfo["status"];

const STORAGE_KEY = "luminode_batches";
const POLL_INTERVAL = 3000;

function loadStored(): StoredBatch[] {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "[]");
  } catch {
    return [];
  }
}

function saveStored(batches: StoredBatch[]) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(batches));
}

// ── Helpers ───────────────────────────────────────────────────────────────────

function fmtTime(epoch: number) {
  return new Date(epoch).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function statusColor(s: JobInfo["status"]) {
  return s === "done"
    ? "#4ade80"
    : s === "running"
      ? "#60a5fa"
      : s === "failed"
        ? "#f87171"
        : "rgba(255,255,255,0.25)";
}

function statusIcon(s: JobInfo["status"]) {
  return s === "done" ? "✓" : s === "running" ? "⟳" : s === "failed" ? "✗" : "·";
}

const STATUS_ORDER: Record<JobInfo["status"], number> = {
  running: 0,
  pending: 1,
  failed: 2,
  done: 3,
  cancelled: 4,
};

// ── JobRow ────────────────────────────────────────────────────────────────────

function JobRow({
  job,
  onCancelled,
}: {
  job: JobInfo;
  onCancelled?: (job_id: string) => void;
}) {
  const [logOpen, setCancelling] = useState(false);
  const [cancelling, setActuallyCancelling] = useState(false);
  const color = statusColor(job.status);
  const icon = statusIcon(job.status);
  const spin = job.status === "running";
  const canCancel = job.status === "pending" || job.status === "running";

  const handleCancel = async () => {
    setActuallyCancelling(true);
    try {
      const res = await fetch(`/api/luminode/job/${job.job_id}/cancel`, { method: "POST" });
      if (res.ok) onCancelled?.(job.job_id);
    } catch { /* will update on next poll */ }
    finally { setActuallyCancelling(false); }
  };

  return (
    <div
      className="border-b"
      style={{ borderColor: "rgba(255,255,255,0.04)" }}
    >
      <div className="flex items-start gap-2 px-3 py-1.5">
        <span
          className={`shrink-0 text-[11px] w-3 text-center mt-px ${spin ? "animate-spin" : ""}`}
          style={{ color }}
        >
          {icon}
        </span>
        <div className="flex-1 min-w-0">
          <p
            className="text-[11px] truncate"
            style={{
              color: job.status === "done" ? "rgba(255,255,255,0.7)" : "rgba(255,255,255,0.45)",
            }}
          >
            {job.title ?? job.spotify_id}
          </p>
          {job.error && (
            <button
              onClick={() => setCancelling((v) => !v)}
              className="text-left text-[10px] mt-0.5"
              style={{ color: "#f87171" }}
            >
              {logOpen ? "▲ hide log" : "▼ show log"}
            </button>
          )}
        </div>
        {canCancel ? (
          <button
            onClick={handleCancel}
            disabled={cancelling}
            className="shrink-0 text-[9px] font-semibold uppercase tracking-wide mt-0.5 px-1 rounded"
            style={{
              color: "#f87171",
              background: "rgba(248,113,113,0.12)",
              opacity: cancelling ? 0.5 : 1,
            }}
            title="Cancel job"
          >
            {cancelling ? "…" : "cancel"}
          </button>
        ) : (
          <span
            className="shrink-0 text-[9px] font-semibold uppercase tracking-wide mt-0.5"
            style={{ color }}
          >
            {job.status}
          </span>
        )}
      </div>

      {/* Expandable error log */}
      {logOpen && job.error && (
        <div className="px-3 pb-2">
          <pre
            className="text-[10px] rounded-md p-2 overflow-x-auto whitespace-pre-wrap wrap-break-word"
            style={{
              background: "rgba(248,113,113,0.08)",
              border: "1px solid rgba(248,113,113,0.2)",
              color: "#fca5a5",
              maxHeight: 180,
              overflowY: "auto",
            }}
          >
            {job.error}
          </pre>
        </div>
      )}
    </div>
  );
}

// ── BatchCard ─────────────────────────────────────────────────────────────────

function BatchCard({
  stored,
  status,
  accentColor,
  onRemove,
  onJobCancelled,
}: {
  stored: StoredBatch;
  status: BatchStatus | null;
  accentColor?: string;
  onRemove: () => void;
  onJobCancelled: (job_id: string) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const [filter, setFilter] = useState<StatusFilter>("all");
  const accent = accentColor ?? "#60a5fa";

  const total = status?.total ?? stored.total;
  const done = status?.done ?? 0;
  const failed = status?.failed ?? 0;
  const running = status?.running ?? 0;
  const pending = status?.pending ?? total - done - failed - running;
  const pct = total > 0 ? Math.round((done / total) * 100) : 0;
  const isComplete = status != null && done + failed >= total;
  const barColor = failed > 0 ? "#f87171" : accent;

  const filteredJobs = status
    ? [...status.jobs]
        .filter((j) => filter === "all" || j.status === filter)
        .sort((a, b) => STATUS_ORDER[a.status] - STATUS_ORDER[b.status])
    : [];

  type TabEntry = { key: StatusFilter; label: string; count: number; color: string };
  const filterTabs: TabEntry[] = ([
    { key: "all"     as StatusFilter, label: "All",     count: total,   color: "rgba(255,255,255,0.4)" },
    { key: "running" as StatusFilter, label: "Running", count: running, color: "#60a5fa" },
    { key: "pending" as StatusFilter, label: "Pending", count: pending, color: "rgba(255,255,255,0.3)" },
    { key: "failed"  as StatusFilter, label: "Failed",  count: failed,  color: "#f87171" },
    { key: "done"    as StatusFilter, label: "Done",    count: done,    color: "#4ade80" },
  ] as TabEntry[]).filter((t) => t.key === "all" || t.count > 0);

  return (
    <div
      className="rounded-md mb-2 overflow-hidden"
      style={{
        background: "rgba(255,255,255,0.04)",
        border: "1px solid rgba(255,255,255,0.07)",
      }}
    >
      {/* Header */}
      <div className="px-3 py-2">
        <div className="flex items-center gap-2 mb-1.5">
          <span
            className="text-[10px] font-mono truncate flex-1 min-w-0"
            style={{ color: "rgba(255,255,255,0.3)" }}
            title={stored.url}
          >
            {stored.url}
          </span>
          <span className="text-[10px] shrink-0" style={{ color: "rgba(255,255,255,0.2)" }}>
            {fmtTime(stored.submitted_at)}
          </span>
          <button
            onClick={onRemove}
            className="shrink-0 ml-1"
            style={{ color: "rgba(255,255,255,0.2)" }}
            title="Dismiss"
          >
            <svg className="w-3 h-3" viewBox="0 0 24 24" fill="currentColor">
              <path d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z" />
            </svg>
          </button>
        </div>

        {/* Progress bar */}
        <div
          className="w-full h-1.5 rounded-full mb-1.5 overflow-hidden"
          style={{ background: "rgba(255,255,255,0.08)" }}
        >
          <div
            className="h-full rounded-full transition-all duration-500"
            style={{ width: `${pct}%`, background: barColor }}
          />
        </div>

        {/* Stats + toggle */}
        <div className="flex items-center justify-between">
          <div className="flex gap-2.5 text-[10px]">
            <span style={{ color: "#4ade80" }}>{done} done</span>
            {running > 0 && <span style={{ color: "#60a5fa" }}>{running} running</span>}
            {pending > 0 && <span style={{ color: "rgba(255,255,255,0.3)" }}>{pending} pending</span>}
            {failed > 0  && <span style={{ color: "#f87171" }}>{failed} failed</span>}
          </div>
          <div className="flex items-center gap-2">
            <span className="text-[10px]" style={{ color: "rgba(255,255,255,0.3)" }}>
              {pct}% · {total}
            </span>
            {status && (
              <button
                onClick={() => setExpanded((v) => !v)}
                className="text-[10px] px-1.5 py-0.5 rounded"
                style={{ color: "rgba(255,255,255,0.4)", background: "rgba(255,255,255,0.06)" }}
              >
                {expanded ? "hide" : "jobs"}
              </button>
            )}
          </div>
        </div>

        {isComplete && (
          <div className="mt-1 text-[10px]" style={{ color: failed > 0 ? "#f87171" : "#4ade80" }}>
            {failed > 0 ? `Completed with ${failed} errors` : "All done"}
          </div>
        )}
      </div>

      {/* Expanded job list */}
      {expanded && status && (
        <div className="border-t" style={{ borderColor: "rgba(255,255,255,0.06)" }}>
          {/* Filter tabs */}
          <div className="flex gap-1 px-2 py-1.5 flex-wrap">
            {filterTabs.map((t) => (
              <button
                key={t.key}
                onClick={() => setFilter(t.key)}
                className="text-[9px] font-semibold uppercase tracking-wide px-1.5 py-0.5 rounded"
                style={{
                  color: filter === t.key ? "#000" : t.color,
                  background: filter === t.key ? t.color : "rgba(255,255,255,0.06)",
                }}
              >
                {t.label} {t.count}
              </button>
            ))}
          </div>
          <div style={{ maxHeight: 280, overflowY: "auto" }}>
            {filteredJobs.map((job) => (
              <JobRow key={job.job_id} job={job} onCancelled={onJobCancelled} />
            ))}
            {filteredJobs.length === 0 && (
              <p className="px-3 py-2 text-[11px]" style={{ color: "rgba(255,255,255,0.2)" }}>
                No {filter} jobs
              </p>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

// ── AllJobsView ───────────────────────────────────────────────────────────────

function AllJobsView({
  statuses,
  accentColor,
  onJobCancelled,
}: {
  statuses: Record<string, BatchStatus>;
  accentColor?: string;
  onJobCancelled: (job_id: string) => void;
}) {
  const [filter, setFilter] = useState<StatusFilter>("all");
  const [query, setQuery] = useState("");
  const accent = accentColor ?? "#60a5fa";

  const allJobs = Object.values(statuses).flatMap((s) => s.jobs);
  const counts = {
    all: allJobs.length,
    running: allJobs.filter((j) => j.status === "running").length,
    pending: allJobs.filter((j) => j.status === "pending").length,
    failed:  allJobs.filter((j) => j.status === "failed").length,
    done:    allJobs.filter((j) => j.status === "done").length,
  };

  const filtered = allJobs
    .filter((j) => filter === "all" || j.status === filter)
    .filter((j) => {
      if (!query) return true;
      const q = query.toLowerCase();
      return (
        (j.title ?? "").toLowerCase().includes(q) ||
        j.spotify_id.toLowerCase().includes(q)
      );
    })
    .sort((a, b) => STATUS_ORDER[a.status] - STATUS_ORDER[b.status]);

  const filterTabs: { key: StatusFilter; label: string; count: number; color: string }[] = [
    { key: "all" as const,     label: "All",     count: counts.all,     color: "rgba(255,255,255,0.4)" },
    { key: "running" as const, label: "Running", count: counts.running, color: "#60a5fa" },
    { key: "pending" as const, label: "Pending", count: counts.pending, color: "rgba(255,255,255,0.3)" },
    { key: "failed" as const,  label: "Failed",  count: counts.failed,  color: "#f87171" },
    { key: "done" as const,    label: "Done",    count: counts.done,    color: "#4ade80" },
  ];

  return (
    <div className="flex flex-col flex-1 min-h-0">
      {/* Filter tabs */}
      <div className="px-3 pt-2 pb-1 shrink-0 flex gap-1 flex-wrap">
        {filterTabs.map((t) => (
          <button
            key={t.key}
            onClick={() => setFilter(t.key)}
            className="text-[9px] font-semibold uppercase tracking-wide px-1.5 py-0.5 rounded transition-colors"
            style={{
              color: filter === t.key ? "#000" : t.color,
              background: filter === t.key ? t.color : "rgba(255,255,255,0.06)",
            }}
          >
            {t.label} {t.count}
          </button>
        ))}
      </div>

      {/* Search */}
      <div className="px-3 pb-2 shrink-0">
        <input
          type="text"
          placeholder="Filter by title…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          className="w-full text-[11px] px-2.5 py-1.5 rounded-md outline-none"
          style={{
            background: "rgba(255,255,255,0.05)",
            border: "1px solid rgba(255,255,255,0.08)",
            color: "rgba(255,255,255,0.7)",
            caretColor: accent,
          }}
        />
      </div>

      {/* Job list */}
      <div className="flex-1 overflow-y-auto">
        {filtered.length === 0 && (
          <p className="px-4 py-4 text-[11px]" style={{ color: "rgba(255,255,255,0.2)" }}>
            {allJobs.length === 0 ? "No jobs yet." : "No matching jobs."}
          </p>
        )}
        {filtered.map((job) => (
          <JobRow key={job.job_id} job={job} onCancelled={onJobCancelled} />
        ))}
      </div>
    </div>
  );
}

// ── Main component ────────────────────────────────────────────────────────────

export default function BatchJobs({ accentColor }: { accentColor?: string }) {
  const [url, setUrl]                 = useState("");
  const [submitting, setSubmitting]   = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [stored, setStored]           = useState<StoredBatch[]>([]);
  const [statuses, setStatuses]       = useState<Record<string, BatchStatus>>({});
  const [viewMode, setViewMode]       = useState<ViewMode>("batches");
  const accent = accentColor ?? "#60a5fa";

  useEffect(() => { setStored(loadStored()); }, []);

  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const poll = useCallback(async (batches: StoredBatch[]) => {
    const incomplete = batches.filter((b) => {
      const s = statuses[b.batch_id];
      return !s || s.done + s.failed < s.total;
    });
    await Promise.all(
      incomplete.map(async (b) => {
        try {
          const res = await fetch(`/api/luminode/batch/${b.batch_id}`);
          if (res.ok) {
            const data: BatchStatus = await res.json();
            setStatuses((prev) => ({ ...prev, [b.batch_id]: data }));
          }
        } catch { /* keep existing */ }
      }),
    );
  }, [statuses]);

  useEffect(() => {
    if (stored.length === 0) return;
    poll(stored);
    pollRef.current = setInterval(() => poll(stored), POLL_INTERVAL);
    return () => { if (pollRef.current) clearInterval(pollRef.current); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [stored]);

  const handleSubmit = async () => {
    const trimmed = url.trim();
    if (!trimmed) return;
    setSubmitting(true);
    setSubmitError(null);
    try {
      const res = await fetch("/api/luminode/submit", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ url: trimmed }),
      });
      const data = await res.json();
      if (!res.ok) {
        setSubmitError(data.detail ?? data.error ?? "Submission failed");
        return;
      }
      const entry: StoredBatch = {
        batch_id: data.batch_id,
        submitted_at: Date.now(),
        url: trimmed,
        total: data.total,
      };
      const next = [entry, ...stored];
      setStored(next);
      saveStored(next);
      setStatuses((prev) => ({
        ...prev,
        [data.batch_id]: {
          batch_id: data.batch_id,
          total: data.total,
          pending: data.total,
          running: 0,
          done: 0,
          failed: 0,
          jobs: data.jobs,
        },
      }));
      setUrl("");
    } catch (e) {
      setSubmitError(String(e));
    } finally {
      setSubmitting(false);
    }
  };

  const handleJobCancelled = useCallback((job_id: string) => {
    setStatuses((prev) => {
      const next = { ...prev };
      for (const [bid, batch] of Object.entries(next)) {
        const idx = batch.jobs.findIndex((j) => j.job_id === job_id);
        if (idx === -1) continue;
        const oldStatus = batch.jobs[idx].status;
        if (oldStatus !== "pending" && oldStatus !== "running") break;
        const jobs = [...batch.jobs];
        jobs[idx] = { ...jobs[idx], status: "cancelled" };
        const delta = oldStatus === "pending" ? { pending: batch.pending - 1 } : { running: batch.running - 1 };
        next[bid] = { ...batch, jobs, ...delta };
        break;
      }
      return next;
    });
  }, []);

  const removeBatch = (batch_id: string) => {
    const next = stored.filter((b) => b.batch_id !== batch_id);
    setStored(next);
    saveStored(next);
    setStatuses((prev) => {
      const copy = { ...prev };
      delete copy[batch_id];
      return copy;
    });
  };

  const totals = Object.values(statuses).reduce(
    (acc, s) => ({
      total:   acc.total   + s.total,
      done:    acc.done    + s.done,
      failed:  acc.failed  + s.failed,
      running: acc.running + s.running,
      pending: acc.pending + s.pending,
    }),
    { total: 0, done: 0, failed: 0, running: 0, pending: 0 },
  );

  return (
    <div className="flex flex-col h-full overflow-hidden">
      {/* Submission form */}
      <div className="px-3 pt-3 pb-2 shrink-0 flex flex-col gap-2">
        <p className="text-[10px] font-semibold tracking-[0.18em] uppercase"
           style={{ color: "rgba(255,255,255,0.3)" }}>
          Submit Spotify URL
        </p>
        <textarea
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); handleSubmit(); } }}
          placeholder="Track, album, or playlist URL…"
          rows={2}
          className="w-full text-[11px] px-2.5 py-2 rounded-md outline-none resize-none"
          style={{
            background: "rgba(255,255,255,0.06)",
            border: "1px solid rgba(255,255,255,0.08)",
            color: "rgba(255,255,255,0.8)",
            caretColor: accent,
          }}
        />
        {submitError && (
          <p className="text-[10px]" style={{ color: "#f87171" }}>{submitError}</p>
        )}
        <button
          onClick={handleSubmit}
          disabled={submitting || !url.trim()}
          className="w-full py-1.5 rounded-md text-[11px] font-semibold transition-opacity"
          style={{ background: accent, color: "#000", opacity: submitting || !url.trim() ? 0.4 : 1 }}
        >
          {submitting ? "Submitting…" : "Queue Batch"}
        </button>
      </div>

      <div className="h-px mx-3 shrink-0" style={{ background: "rgba(255,255,255,0.06)" }} />

      {/* Global summary + view toggle */}
      {stored.length > 0 && (
        <div className="px-3 py-2 shrink-0 flex items-center justify-between">
          <div className="text-[10px] flex gap-2">
            <span style={{ color: "#4ade80" }}>{totals.done}</span>
            {totals.running > 0 && <span style={{ color: "#60a5fa" }}>{totals.running} running</span>}
            {totals.failed  > 0 && <span style={{ color: "#f87171" }}>{totals.failed} failed</span>}
            <span style={{ color: "rgba(255,255,255,0.25)" }}>/ {totals.total}</span>
          </div>
          <div className="flex gap-1">
            {(["batches", "alljobs"] as ViewMode[]).map((m) => (
              <button
                key={m}
                onClick={() => setViewMode(m)}
                className="text-[9px] font-semibold uppercase tracking-wide px-2 py-0.5 rounded"
                style={{
                  color: viewMode === m ? "#000" : "rgba(255,255,255,0.35)",
                  background: viewMode === m ? accent : "rgba(255,255,255,0.06)",
                }}
              >
                {m === "batches" ? "Batches" : "All Jobs"}
              </button>
            ))}
          </div>
        </div>
      )}

      {/* Content area */}
      {viewMode === "batches" || stored.length === 0 ? (
        <div className="flex-1 overflow-y-auto px-3 pb-3">
          {stored.length === 0 && (
            <p className="text-[12px] pt-4" style={{ color: "rgba(255,255,255,0.2)" }}>
              No batches yet. Paste a Spotify URL above.
            </p>
          )}
          {stored.map((b) => (
            <BatchCard
              key={b.batch_id}
              stored={b}
              status={statuses[b.batch_id] ?? null}
              accentColor={accentColor}
              onRemove={() => removeBatch(b.batch_id)}
              onJobCancelled={handleJobCancelled}
            />
          ))}
        </div>
      ) : (
        <AllJobsView statuses={statuses} accentColor={accentColor} onJobCancelled={handleJobCancelled} />
      )}
    </div>
  );
}
