import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import { execFile } from "node:child_process";
import { mkdir } from "node:fs/promises";
import path from "node:path";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

async function runMallard(args: string[], cwd: string, signal?: AbortSignal) {
  try {
    const { stdout, stderr } = await execFileAsync("mallard", args, {
      cwd,
      signal,
      maxBuffer: 50 * 1024 * 1024,
    });

    let json: unknown = undefined;
    try {
      json = JSON.parse(stdout);
    } catch {
      // Some future command may return plain text. Preserve stdout below.
    }

    return {
      content: [
        {
          type: "text" as const,
          text: json === undefined ? stdout.trim() : JSON.stringify(json, null, 2),
        },
      ],
      details: { args, stdout, stderr, json },
    };
  } catch (error) {
    const e = error as Error & { stdout?: string; stderr?: string; code?: number | string };
    return {
      content: [
        {
          type: "text" as const,
          text: e.stderr?.trim() || e.message,
        },
      ],
      details: { args, stdout: e.stdout, stderr: e.stderr, code: e.code },
      isError: true,
    };
  }
}

function repoPath(ctxCwd: string, value: string) {
  return path.isAbsolute(value) ? value : path.join(ctxCwd, value);
}

export default function (pi: ExtensionAPI) {
  pi.registerCommand("mallard-status", {
    description: "Check whether the mallard binary is available",
    handler: async (_args, ctx) => {
      const result = await runMallard(["--version"], ctx.cwd, ctx.signal);
      const text = result.isError
        ? "mallard binary not found or not runnable"
        : String(result.content[0]?.text ?? "mallard is available");
      ctx.ui.notify(text, result.isError ? "error" : "info");
    },
  });

  pi.registerTool({
    name: "mallard_index",
    label: "Mallard Index",
    description: "Build a Mallard DuckDB code index for a repo path. Requires the mallard binary on PATH.",
    promptSnippet: "Build a deterministic structural code index for the current repository",
    promptGuidelines: [
      "Use mallard_index before Mallard queries when no current index exists or source changed.",
      "Use .mallard/head.duckdb for current working-tree indexes unless the user asks otherwise.",
    ],
    parameters: Type.Object({
      repo: Type.Optional(Type.String({ description: "Repository path, relative to cwd or absolute", default: "." })),
      sha: Type.Optional(Type.String({ description: "Index label/SHA", default: "working-tree" })),
      out: Type.Optional(Type.String({ description: "Output DuckDB path", default: ".mallard/head.duckdb" })),
      languages: Type.Optional(Type.Array(Type.String(), { description: "Optional language allow-list" })),
    }),
    async execute(_toolCallId, params, signal, _onUpdate, ctx) {
      const out = params.out ?? ".mallard/head.duckdb";
      await mkdir(path.dirname(repoPath(ctx.cwd, out)), { recursive: true });
      const args = ["index", "--sha", params.sha ?? "working-tree", "--out", out];
      for (const lang of params.languages ?? []) args.push("--lang", lang);
      args.push(params.repo ?? ".");
      return runMallard(args, ctx.cwd, signal);
    },
  });

  pi.registerTool({
    name: "mallard_find",
    label: "Mallard Find",
    description: "Find symbols by qualified name in a Mallard index.",
    promptSnippet: "Find symbols by qualified name in a Mallard index",
    parameters: Type.Object({
      index: Type.Optional(Type.String({ description: "DuckDB index path", default: ".mallard/head.duckdb" })),
      qname: Type.String({ description: "Qualified name or short symbol name" }),
    }),
    async execute(_toolCallId, params, signal, _onUpdate, ctx) {
      return runMallard(["query", "find", "--index", params.index ?? ".mallard/head.duckdb", "--qname", params.qname], ctx.cwd, signal);
    },
  });

  pi.registerTool({
    name: "mallard_blast_radius",
    label: "Mallard Blast Radius",
    description: "Return a symbol plus callers, callees, test seams, and sibling qname matches.",
    promptSnippet: "Scope what breaks before renaming, removing, or modifying a symbol",
    promptGuidelines: ["Use mallard_blast_radius before proposing a rename, removal, or signature change."],
    parameters: Type.Object({
      index: Type.Optional(Type.String({ description: "DuckDB index path", default: ".mallard/head.duckdb" })),
      qname: Type.String({ description: "Qualified name or short symbol name" }),
    }),
    async execute(_toolCallId, params, signal, _onUpdate, ctx) {
      return runMallard(["query", "blast-radius", "--index", params.index ?? ".mallard/head.duckdb", "--qname", params.qname], ctx.cwd, signal);
    },
  });

  pi.registerTool({
    name: "mallard_test_seams",
    label: "Mallard Test Seams",
    description: "Find tests that exercise a symbol according to the Mallard index.",
    promptSnippet: "Find test seams for a symbol",
    parameters: Type.Object({
      index: Type.Optional(Type.String({ description: "DuckDB index path", default: ".mallard/head.duckdb" })),
      qname: Type.String({ description: "Qualified name or short symbol name" }),
    }),
    async execute(_toolCallId, params, signal, _onUpdate, ctx) {
      return runMallard(["query", "test-seams", "--index", params.index ?? ".mallard/head.duckdb", "--qname", params.qname], ctx.cwd, signal);
    },
  });

  pi.registerTool({
    name: "mallard_symbol_diff",
    label: "Mallard Symbol Diff",
    description: "Compare two Mallard indexes and return added, removed, and modified symbols.",
    promptSnippet: "Compare base/head Mallard indexes for structural symbol changes",
    parameters: Type.Object({
      baseDb: Type.Optional(Type.String({ description: "Base DuckDB index path", default: ".mallard/base.duckdb" })),
      headDb: Type.Optional(Type.String({ description: "Head DuckDB index path", default: ".mallard/head.duckdb" })),
    }),
    async execute(_toolCallId, params, signal, _onUpdate, ctx) {
      return runMallard(["symbol-diff", "--base-db", params.baseDb ?? ".mallard/base.duckdb", "--head-db", params.headDb ?? ".mallard/head.duckdb"], ctx.cwd, signal);
    },
  });

  pi.registerTool({
    name: "mallard_unresolved_callers",
    label: "Mallard Unresolved Callers",
    description: "Find unresolved call sites by name in a Mallard index, useful after deleting or renaming a symbol.",
    promptSnippet: "Find call sites that still reference removed or renamed symbols",
    parameters: Type.Object({
      index: Type.Optional(Type.String({ description: "DuckDB index path", default: ".mallard/head.duckdb" })),
      names: Type.Array(Type.String(), { description: "Removed or renamed symbol names to search for" }),
    }),
    async execute(_toolCallId, params, signal, _onUpdate, ctx) {
      return runMallard(["query", "unresolved-callers", "--index", params.index ?? ".mallard/head.duckdb", "--name", params.names.join(",")], ctx.cwd, signal);
    },
  });
}
