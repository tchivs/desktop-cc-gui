import { useEffect, useMemo, useState } from "react";
import {
  Activity,
  BadgeCheck,
  Clock3,
  Flame,
  Pin,
  RefreshCw,
  Snowflake,
  Sparkles,
  SquareTerminal,
  Trash2,
  TriangleAlert,
} from "lucide-react";
import type { AppSettings, RuntimePoolSnapshot } from "@/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Separator } from "@/components/ui/separator";
import {
  getRuntimePoolSnapshot,
  mutateRuntimePool,
} from "../../../../../services/tauri";

type RuntimePoolSectionProps = {
  t: (key: string, options?: Record<string, unknown>) => string;
  appSettings: AppSettings;
  onUpdateAppSettings: (next: AppSettings) => Promise<void>;
};

function formatTimestamp(value?: number | null) {
  if (!value) {
    return "—";
  }
  try {
    return new Date(value).toLocaleString();
  } catch {
    return "—";
  }
}

function getRuntimeTone(state: string) {
  switch (state.toLowerCase()) {
    case "hot":
      return {
        icon: Flame,
        chip: "bg-orange-500/10 text-orange-700 border-orange-300/60",
      };
    case "warm":
      return {
        icon: Snowflake,
        chip: "bg-sky-500/10 text-sky-700 border-sky-300/60",
      };
    case "busy":
      return {
        icon: Activity,
        chip: "bg-emerald-500/10 text-emerald-700 border-emerald-300/60",
      };
    case "failed":
    case "zombiesuspected":
      return {
        icon: TriangleAlert,
        chip: "bg-red-500/10 text-red-700 border-red-300/60",
      };
    default:
      return {
        icon: SquareTerminal,
        chip: "bg-slate-500/10 text-slate-700 border-slate-300/60",
      };
  }
}

export function RuntimePoolSection({
  t,
  appSettings,
  onUpdateAppSettings,
}: RuntimePoolSectionProps) {
  const [runtimeSnapshot, setRuntimeSnapshot] = useState<RuntimePoolSnapshot | null>(null);
  const [runtimeLoading, setRuntimeLoading] = useState(false);
  const [runtimeError, setRuntimeError] = useState<string | null>(null);
  const [runtimeSaving, setRuntimeSaving] = useState(false);
  const [hotDraft, setHotDraft] = useState(String(appSettings.codexMaxHotRuntimes ?? 1));
  const [warmDraft, setWarmDraft] = useState(String(appSettings.codexMaxWarmRuntimes ?? 1));
  const [ttlDraft, setTtlDraft] = useState(String(appSettings.codexWarmTtlSeconds ?? 90));

  useEffect(() => {
    setHotDraft(String(appSettings.codexMaxHotRuntimes ?? 1));
    setWarmDraft(String(appSettings.codexMaxWarmRuntimes ?? 1));
    setTtlDraft(String(appSettings.codexWarmTtlSeconds ?? 90));
  }, [
    appSettings.codexMaxHotRuntimes,
    appSettings.codexMaxWarmRuntimes,
    appSettings.codexWarmTtlSeconds,
  ]);

  const loadSnapshot = async () => {
    setRuntimeLoading(true);
    setRuntimeError(null);
    try {
      setRuntimeSnapshot(await getRuntimePoolSnapshot());
    } catch (error) {
      setRuntimeError(error instanceof Error ? error.message : String(error));
    } finally {
      setRuntimeLoading(false);
    }
  };

  useEffect(() => {
    void loadSnapshot();
  }, []);

  const summaryCards = useMemo(() => {
    const summary = runtimeSnapshot?.summary;
    return [
      {
        key: "total",
        icon: SquareTerminal,
        value: summary?.totalRuntimes ?? 0,
        label: t("settings.runtimeMetricTotal"),
        accent: "from-slate-500/15 to-slate-400/5",
      },
      {
        key: "hot",
        icon: Flame,
        value: summary?.hotRuntimes ?? 0,
        label: t("settings.runtimeMetricHot"),
        accent: "from-orange-500/15 to-orange-400/5",
      },
      {
        key: "warm",
        icon: Snowflake,
        value: summary?.warmRuntimes ?? 0,
        label: t("settings.runtimeMetricWarm"),
        accent: "from-sky-500/15 to-sky-400/5",
      },
      {
        key: "busy",
        icon: Activity,
        value: summary?.busyRuntimes ?? 0,
        label: t("settings.runtimeMetricBusy"),
        accent: "from-emerald-500/15 to-emerald-400/5",
      },
      {
        key: "pinned",
        icon: Pin,
        value: summary?.pinnedRuntimes ?? 0,
        label: t("settings.runtimeMetricPinned"),
        accent: "from-violet-500/15 to-violet-400/5",
      },
    ];
  }, [runtimeSnapshot?.summary, t]);

  const handleRuntimeMutation = async (
    action: "close" | "releaseToCold" | "pin",
    workspaceId: string,
    pinned?: boolean,
  ) => {
    setRuntimeSaving(true);
    setRuntimeError(null);
    try {
      const snapshot = await mutateRuntimePool({ action, workspaceId, pinned });
      setRuntimeSnapshot(snapshot);
    } catch (error) {
      setRuntimeError(error instanceof Error ? error.message : String(error));
    } finally {
      setRuntimeSaving(false);
    }
  };

  const handleSaveRuntimeSettings = async () => {
    const nextHot = Number.parseInt(hotDraft, 10);
    const nextWarm = Number.parseInt(warmDraft, 10);
    const nextTtl = Number.parseInt(ttlDraft, 10);
    setRuntimeSaving(true);
    setRuntimeError(null);
    try {
      await onUpdateAppSettings({
        ...appSettings,
        codexMaxHotRuntimes: Number.isFinite(nextHot)
          ? Math.max(0, Math.min(8, nextHot))
          : 1,
        codexMaxWarmRuntimes: Number.isFinite(nextWarm)
          ? Math.max(0, Math.min(16, nextWarm))
          : 1,
        codexWarmTtlSeconds: Number.isFinite(nextTtl)
          ? Math.max(15, Math.min(3600, nextTtl))
          : 90,
      });
      await loadSnapshot();
    } catch (error) {
      setRuntimeError(error instanceof Error ? error.message : String(error));
    } finally {
      setRuntimeSaving(false);
    }
  };

  return (
    <section className="settings-section">
      <div className="settings-section-title">{t("settings.runtimePanelTitle")}</div>
      <div className="settings-section-subtitle">
        {t("settings.runtimePanelDescription")}
      </div>

      <Card className="border-slate-200/80 bg-gradient-to-br from-white via-slate-50/80 to-slate-100/70 shadow-sm">
        <CardHeader className="gap-4 md:flex-row md:items-start md:justify-between">
          <div className="space-y-3">
            <div className="flex items-center gap-3">
              <div className="flex h-11 w-11 items-center justify-center rounded-2xl bg-slate-900 text-white shadow-sm">
                <SquareTerminal size={20} />
              </div>
              <div>
                <CardTitle className="text-xl">{t("settings.runtimePoolTitle")}</CardTitle>
                <CardDescription className="mt-1 text-sm leading-6">
                  {t("settings.runtimePoolDescription")}
                </CardDescription>
              </div>
            </div>
            <div className="flex flex-wrap gap-2">
              <Badge variant="outline">{`Hot ${appSettings.codexMaxHotRuntimes}`}</Badge>
              <Badge variant="outline">{`Warm ${appSettings.codexMaxWarmRuntimes}`}</Badge>
              <Badge variant="outline">{`TTL ${appSettings.codexWarmTtlSeconds}s`}</Badge>
            </div>
          </div>
          <div className="flex gap-2">
            <Button
              type="button"
              variant="outline"
              onClick={() => {
                void loadSnapshot();
              }}
              disabled={runtimeLoading}
            >
              <RefreshCw className="mr-2 h-4 w-4" />
              {t("settings.refresh")}
            </Button>
          </div>
        </CardHeader>
      </Card>

      <div className="mt-4 grid gap-3 md:grid-cols-5">
        {summaryCards.map((item) => {
          const Icon = item.icon;
          return (
            <Card
              key={item.key}
              className={`overflow-hidden border-slate-200/70 bg-gradient-to-br ${item.accent}`}
            >
              <CardContent className="flex items-start justify-between p-4">
                <div>
                  <div className="text-xs font-medium uppercase tracking-[0.18em] text-slate-500">
                    {item.label}
                  </div>
                  <div className="mt-2 text-3xl font-semibold text-slate-900">
                    {item.value}
                  </div>
                </div>
                <div className="rounded-xl border border-white/60 bg-white/80 p-2 text-slate-700 shadow-sm">
                  <Icon className="h-4 w-4" />
                </div>
              </CardContent>
            </Card>
          );
        })}
      </div>

      <div className="mt-4 grid gap-4 lg:grid-cols-[1.1fr_0.9fr]">
        <Card className="border-slate-200/70">
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <BadgeCheck className="h-4 w-4 text-emerald-600" />
              {t("settings.runtimePolicyTitle")}
            </CardTitle>
            <CardDescription>{t("settings.runtimePolicyDescription")}</CardDescription>
          </CardHeader>
          <CardContent className="grid gap-3">
            <div className="rounded-2xl border border-slate-200/80 bg-slate-50/80 p-4">
              <div className="flex items-start justify-between gap-3">
                <div>
                  <div className="font-medium text-slate-900">
                    {t("settings.runtimeRestoreThreadsOnlyOnLaunch")}
                  </div>
                  <div className="mt-1 text-sm leading-6 text-slate-500">
                    {t("settings.runtimeRestoreThreadsOnlyOnLaunchDesc")}
                  </div>
                </div>
                <Switch
                  checked={appSettings.runtimeRestoreThreadsOnlyOnLaunch !== false}
                  onCheckedChange={(checked) =>
                    void onUpdateAppSettings({
                      ...appSettings,
                      runtimeRestoreThreadsOnlyOnLaunch: checked,
                    })
                  }
                />
              </div>
            </div>
            <div className="rounded-2xl border border-slate-200/80 bg-slate-50/80 p-4">
              <div className="flex items-start justify-between gap-3">
                <div>
                  <div className="font-medium text-slate-900">
                    {t("settings.runtimeForceCleanupOnExit")}
                  </div>
                  <div className="mt-1 text-sm leading-6 text-slate-500">
                    {t("settings.runtimeForceCleanupOnExitDesc")}
                  </div>
                </div>
                <Switch
                  checked={appSettings.runtimeForceCleanupOnExit !== false}
                  onCheckedChange={(checked) =>
                    void onUpdateAppSettings({
                      ...appSettings,
                      runtimeForceCleanupOnExit: checked,
                    })
                  }
                />
              </div>
            </div>
            <div className="rounded-2xl border border-slate-200/80 bg-slate-50/80 p-4">
              <div className="flex items-start justify-between gap-3">
                <div>
                  <div className="font-medium text-slate-900">
                    {t("settings.runtimeOrphanSweepOnLaunch")}
                  </div>
                  <div className="mt-1 text-sm leading-6 text-slate-500">
                    {t("settings.runtimeOrphanSweepOnLaunchDesc")}
                  </div>
                </div>
                <Switch
                  checked={appSettings.runtimeOrphanSweepOnLaunch !== false}
                  onCheckedChange={(checked) =>
                    void onUpdateAppSettings({
                      ...appSettings,
                      runtimeOrphanSweepOnLaunch: checked,
                    })
                  }
                />
              </div>
            </div>
          </CardContent>
        </Card>

        <Card className="border-slate-200/70">
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Sparkles className="h-4 w-4 text-violet-600" />
              {t("settings.runtimeBudgetTitle")}
            </CardTitle>
            <CardDescription>{t("settings.runtimeBudgetDescription")}</CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="grid gap-3 md:grid-cols-3">
              <div className="space-y-2">
                <Label htmlFor="runtime-hot">{t("settings.runtimeMaxHot")}</Label>
                <Input
                  id="runtime-hot"
                  value={hotDraft}
                  onChange={(event) => setHotDraft(event.target.value)}
                />
                <div className="text-xs leading-5 text-slate-500">
                  {t("settings.runtimeMaxHotHelp")}
                </div>
              </div>
              <div className="space-y-2">
                <Label htmlFor="runtime-warm">{t("settings.runtimeMaxWarm")}</Label>
                <Input
                  id="runtime-warm"
                  value={warmDraft}
                  onChange={(event) => setWarmDraft(event.target.value)}
                />
                <div className="text-xs leading-5 text-slate-500">
                  {t("settings.runtimeMaxWarmHelp")}
                </div>
              </div>
              <div className="space-y-2">
                <Label htmlFor="runtime-ttl">{t("settings.runtimeWarmTtl")}</Label>
                <Input
                  id="runtime-ttl"
                  value={ttlDraft}
                  onChange={(event) => setTtlDraft(event.target.value)}
                />
                <div className="text-xs leading-5 text-slate-500">
                  {t("settings.runtimeWarmTtlHelp")}
                </div>
              </div>
            </div>
            <div className="flex gap-2">
              <Button
                type="button"
                onClick={() => {
                  void handleSaveRuntimeSettings();
                }}
                disabled={runtimeSaving}
              >
                {runtimeSaving ? t("settings.running") : t("common.save")}
              </Button>
              <Button
                type="button"
                variant="outline"
                onClick={() => {
                  void loadSnapshot();
                }}
                disabled={runtimeLoading}
              >
                {t("settings.refresh")}
              </Button>
            </div>
          </CardContent>
        </Card>
      </div>

      {runtimeError ? (
        <Card className="mt-4 border-red-200 bg-red-50/60">
          <CardContent className="flex items-start gap-3 p-4 text-red-700">
            <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />
            <div className="text-sm">{runtimeError}</div>
          </CardContent>
        </Card>
      ) : null}

      <Card className="mt-4 border-slate-200/70">
        <CardHeader className="space-y-3">
          <div className="flex items-start justify-between gap-3">
            <div>
              <CardTitle className="text-base">{t("settings.runtimeRowsTitle")}</CardTitle>
              <CardDescription>{t("settings.runtimeRowsDescription")}</CardDescription>
            </div>
            {runtimeSnapshot ? (
              <Badge variant="secondary">
                {t("settings.runtimeDiagnosticsLine", {
                  cleaned: runtimeSnapshot.diagnostics.orphanEntriesCleaned,
                  failed: runtimeSnapshot.diagnostics.orphanEntriesFailed,
                  forced: runtimeSnapshot.diagnostics.forceKillCount,
                })}
              </Badge>
            ) : null}
          </div>
          <Separator />
        </CardHeader>
        <CardContent className="space-y-4">
          {runtimeSnapshot?.rows.length ? (
            runtimeSnapshot.rows.map((row) => {
              const tone = getRuntimeTone(row.state);
              const StatusIcon = tone.icon;
              return (
                <div
                  key={`${row.engine}:${row.workspaceId}`}
                  className="rounded-3xl border border-slate-200/80 bg-white p-5 shadow-sm"
                >
                  <div className="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
                    <div className="min-w-0 flex-1 space-y-3">
                      <div className="flex flex-wrap items-center gap-2">
                        <div className="flex h-9 w-9 items-center justify-center rounded-2xl bg-slate-900 text-white">
                          <StatusIcon className="h-4 w-4" />
                        </div>
                        <div>
                          <div className="font-semibold text-slate-900">
                            {row.workspaceName}
                          </div>
                          <div className="text-xs uppercase tracking-[0.18em] text-slate-500">
                            {row.engine}
                          </div>
                        </div>
                        <Badge className={tone.chip}>{row.state}</Badge>
                        {row.pinned ? <Badge variant="secondary">{t("settings.runtimePin")}</Badge> : null}
                      </div>

                      <div className="grid gap-2 text-sm text-slate-600 lg:grid-cols-2">
                        <div>
                          <span className="font-medium text-slate-900">
                            {t("settings.runtimePathLabel")}
                          </span>{" "}
                          {row.workspacePath}
                        </div>
                        <div>
                          <span className="font-medium text-slate-900">
                            {t("settings.runtimeLeaseSourcesLabel")}
                          </span>{" "}
                          {row.leaseSources.join(" · ") || "—"}
                        </div>
                        <div>
                          <span className="font-medium text-slate-900">
                            {t("settings.runtimeProcessLabel")}
                          </span>{" "}
                          {row.pid ? `pid ${row.pid}` : "—"}
                          {row.wrapperKind ? ` · ${row.wrapperKind}` : ""}
                        </div>
                        <div>
                          <span className="font-medium text-slate-900">
                            {t("settings.runtimeBinaryLabel")}
                          </span>{" "}
                          {row.resolvedBin ?? "—"}
                        </div>
                      </div>

                      <div className="flex flex-wrap gap-3 text-xs text-slate-500">
                        <div className="inline-flex items-center gap-1.5">
                          <Clock3 className="h-3.5 w-3.5" />
                          {t("settings.runtimeStartedAtLabel")} {formatTimestamp(row.startedAtMs)}
                        </div>
                        <div className="inline-flex items-center gap-1.5">
                          <RefreshCw className="h-3.5 w-3.5" />
                          {t("settings.runtimeLastUsedLabel")} {formatTimestamp(row.lastUsedAtMs)}
                        </div>
                      </div>

                      {row.error ? (
                        <div className="rounded-2xl border border-red-200 bg-red-50/80 px-3 py-2 text-sm text-red-700">
                          {row.error}
                        </div>
                      ) : null}
                    </div>

                    <div className="flex shrink-0 flex-wrap gap-2 xl:w-[220px] xl:justify-end">
                      <Button
                        type="button"
                        variant="outline"
                        onClick={() => {
                          void handleRuntimeMutation("pin", row.workspaceId, !row.pinned);
                        }}
                        disabled={runtimeSaving}
                      >
                        <Pin className="mr-2 h-4 w-4" />
                        {row.pinned
                          ? t("settings.runtimeUnpin")
                          : t("settings.runtimePin")}
                      </Button>
                      <Button
                        type="button"
                        variant="outline"
                        onClick={() => {
                          void handleRuntimeMutation("releaseToCold", row.workspaceId);
                        }}
                        disabled={runtimeSaving}
                      >
                        <Snowflake className="mr-2 h-4 w-4" />
                        {t("settings.runtimeRelease")}
                      </Button>
                      <Button
                        type="button"
                        variant="outline"
                        onClick={() => {
                          void handleRuntimeMutation("close", row.workspaceId);
                        }}
                        disabled={runtimeSaving}
                      >
                        <Trash2 className="mr-2 h-4 w-4" />
                        {t("settings.runtimeClose")}
                      </Button>
                    </div>
                  </div>
                </div>
              );
            })
          ) : (
            <div className="rounded-3xl border border-dashed border-slate-200 bg-slate-50/70 px-6 py-10 text-center">
              <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-2xl bg-white shadow-sm">
                <SquareTerminal className="h-5 w-5 text-slate-500" />
              </div>
              <div className="mt-4 text-sm font-medium text-slate-900">
                {t("settings.runtimePoolEmpty")}
              </div>
              <div className="mt-2 text-sm leading-6 text-slate-500">
                {t("settings.runtimeEmptyDescription")}
              </div>
            </div>
          )}
        </CardContent>
      </Card>
    </section>
  );
}
