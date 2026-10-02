import { invoke } from "@tauri-apps/api/core";

// Thin, explicit wrapper around every Tauri command — one line per
// backend command in commands.rs, so drift between the two is easy to
// spot at a glance.
export const api = {
  listAgents: () => invoke("list_agents"),
  setAgentDisplayName: (agentId, displayName) => invoke("set_agent_display_name", { agentId, displayName }),
  removeAgent: (agentId, addr) => invoke("remove_agent", { agentId, addr }),
  discoverAgents: (timeoutMs = 3000) => invoke("discover_agents", { timeoutMs }),
  discoverAndSyncAll: (timeoutMs = 3000) => invoke("discover_and_sync_all", { timeoutMs }),
  probeAgentByIp: (ip) => invoke("probe_agent_by_ip", { ip }),
  syncAgent: (agentId, addr) => invoke("sync_agent", { agentId, addr }),
  refreshAgentsStatus: () => invoke("refresh_agents_status"),
  updateAgent: (agentId, addr) => invoke("update_agent", { agentId, addr }),
  updateAllOutdatedAgents: () => invoke("update_all_outdated_agents"),

  listGroups: () => invoke("list_groups"),
  createGroup: (name) => invoke("create_group", { name }),
  deleteGroup: (groupId) => invoke("delete_group", { groupId }),
  addGroupMember: (groupId, agentId) => invoke("add_group_member", { groupId, agentId }),
  removeGroupMember: (groupId, agentId) => invoke("remove_group_member", { groupId, agentId }),
  agentGroupIds: (agentId) => invoke("agent_group_ids", { agentId }),

  listLists: () => invoke("list_lists"),
  createList: (name, domains) => invoke("create_list", { name, domains }),
  updateListDomains: (listId, domains) => invoke("update_list_domains", { listId, domains }),
  deleteList: (listId) => invoke("delete_list", { listId }),

  assignmentsFor: (agentId) => invoke("assignments_for", { agentId }),
  listAssignments: () => invoke("list_assignments"),
  listExceptions: () => invoke("list_exceptions"),
  createAssignment: (listId, targetKind, targetId, schedule) =>
    invoke("create_assignment", { listId, targetKind, targetId, schedule }),
  deleteAssignment: (assignmentId) => invoke("delete_assignment", { assignmentId }),
  updateAssignmentSchedule: (assignmentId, schedule) => invoke("update_assignment_schedule", { assignmentId, schedule }),

  exceptionsFor: (agentId) => invoke("exceptions_for", { agentId }),
  createException: (targetKind, targetId, domains, until) =>
    invoke("create_exception", { targetKind, targetId, domains, until }),
  deleteException: (exceptionId) => invoke("delete_exception", { exceptionId }),

  recentHistory: (limit = 100) => invoke("recent_history", { limit }),
  upcomingTransitions: (limit = 8) => invoke("upcoming_transitions", { limit }),
  fleetTimeline: (hours = 24) => invoke("fleet_timeline", { hours }),

  startEnrollment: (ttlSecs, defaultGroupId) => invoke("start_enrollment", { ttlSecs, defaultGroupId }),
  stopEnrollment: () => invoke("stop_enrollment"),
  bundledAgentVersion: () => invoke("bundled_agent_version"),

  caFingerprint: () => invoke("ca_fingerprint"),
  caDetails: () => invoke("ca_details"),
  getTimezone: () => invoke("get_timezone"),
  setTimezone: (timezone) => invoke("set_timezone", { timezone }),
  getTheme: () => invoke("get_theme"),
  setTheme: (theme) => invoke("set_theme", { theme }),
  getOnboardingDone: () => invoke("get_onboarding_done"),
  setOnboardingDone: () => invoke("set_onboarding_done"),
  getScanOnLaunch: () => invoke("get_scan_on_launch"),
  setScanOnLaunch: (enabled) => invoke("set_scan_on_launch", { enabled }),
  getAutoUpdateAgents: () => invoke("get_auto_update_agents"),
  setAutoUpdateAgents: (enabled) => invoke("set_auto_update_agents", { enabled }),
  getDefaultUnblockMinutes: () => invoke("get_default_unblock_minutes"),
  setDefaultUnblockMinutes: (minutes) => invoke("set_default_unblock_minutes", { minutes }),
  getAutoWww: () => invoke("get_auto_www"),
  setAutoWww: (enabled) => invoke("set_auto_www", { enabled }),

  listAliases: () => invoke("list_aliases"),
  upsertAlias: (baseDomain, variants) => invoke("upsert_alias", { baseDomain, variants }),
  deleteAlias: (baseDomain) => invoke("delete_alias", { baseDomain }),

  writeTextFile: (path, contents) => invoke("write_text_file", { path, contents }),
  exportConfig: (path, passphrase) => invoke("export_config", { path, passphrase }),
  importConfig: (path, passphrase) => invoke("import_config", { path, passphrase }),
};
