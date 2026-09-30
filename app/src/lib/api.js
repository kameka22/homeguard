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
  createAssignment: (listId, targetKind, targetId, schedule) =>
    invoke("create_assignment", { listId, targetKind, targetId, schedule }),
  deleteAssignment: (assignmentId) => invoke("delete_assignment", { assignmentId }),

  exceptionsFor: (agentId) => invoke("exceptions_for", { agentId }),
  createException: (targetKind, targetId, domains, until) =>
    invoke("create_exception", { targetKind, targetId, domains, until }),
  deleteException: (exceptionId) => invoke("delete_exception", { exceptionId }),

  recentHistory: (limit = 100) => invoke("recent_history", { limit }),
  upcomingTransitions: (limit = 8) => invoke("upcoming_transitions", { limit }),

  startEnrollment: (ttlSecs, defaultGroupId) => invoke("start_enrollment", { ttlSecs, defaultGroupId }),
  stopEnrollment: () => invoke("stop_enrollment"),
  bundledAgentVersion: () => invoke("bundled_agent_version"),

  caFingerprint: () => invoke("ca_fingerprint"),
  getTimezone: () => invoke("get_timezone"),
  setTimezone: (timezone) => invoke("set_timezone", { timezone }),

  exportConfig: (path, passphrase) => invoke("export_config", { path, passphrase }),
  importConfig: (path, passphrase) => invoke("import_config", { path, passphrase }),
};
