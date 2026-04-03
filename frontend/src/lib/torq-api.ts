import { invoke } from '@tauri-apps/api/core';
import type {
  RuntimeConfigDto,
  RuntimeConfigRequest,
  RuntimeConfigResponse,
  RuntimeControlAuth,
  RuntimeControlAuthCookie,
  RuntimeControlAuthNull,
  RuntimeControlConfig,
  RuntimeLogMode,
} from './runtime-config';

export type {
  RuntimeConfigDto,
  RuntimeConfigRequest,
  RuntimeConfigResponse,
  RuntimeControlAuth,
  RuntimeControlAuthCookie,
  RuntimeControlAuthNull,
  RuntimeControlConfig,
  RuntimeLogMode,
};

export const TOR_STATE_COMMAND = 'tor_state';
export const TOR_RUNTIME_SNAPSHOT_COMMAND = 'tor_runtime_snapshot';
export const TOR_START_COMMAND = 'tor_start';
export const TOR_STOP_COMMAND = 'tor_stop';
export const TOR_RESTART_COMMAND = 'tor_restart';
export const TOR_NEW_IDENTITY_COMMAND = 'tor_new_identity';
export const GET_RUNTIME_CONFIG_COMMAND = 'get_runtime_config';
export const SET_RUNTIME_CONFIG_COMMAND = 'set_runtime_config';
export const TOR_STATE_EVENT = 'tor://state';
export const TOR_RUNTIME_SNAPSHOT_EVENT = 'tor://runtime-snapshot';
export const TOR_ACTIVITY_EVENT = 'tor://activity';

export type RuntimeStatus = 'stopped' | 'starting' | 'running' | 'failed';
export type ControlAvailability = 'unconfigured' | 'unavailable' | 'available';

export interface TorStateDto {
  status: RuntimeStatus;
  bootstrap: number;
}

export interface TorControlRuntimeDto {
  port: ControlAvailability;
  bootstrap_observation: ControlAvailability;
}

export interface TorRuntimeSnapshotDto {
  tor: TorStateDto;
  control: TorControlRuntimeDto;
  control_configured: boolean;
  control_available: boolean;
  bootstrap_observation_available: boolean;
  new_identity_available: boolean;
  uses_control_bootstrap_observation: boolean;
}

export type ActivityTone = 'success' | 'warning' | 'danger' | 'neutral' | 'info';

export interface TorActivityEventDto {
  id?: number;
  kind?: string;
  title?: string;
  details?: string;
  tone?: ActivityTone;
  timestamp?: number | string;
  timestamp_ms?: number;
  progress?: number;
  bootstrap?: number;
  message?: string;
  availability?: ControlAvailability;
  status?: RuntimeStatus;
  coalesce_key?: string;
}

export async function fetchTorState() {
  return invoke<TorStateDto>(TOR_STATE_COMMAND);
}

export async function fetchTorRuntimeSnapshot() {
  return invoke<TorRuntimeSnapshotDto>(TOR_RUNTIME_SNAPSHOT_COMMAND);
}

export async function startTor() {
  return invoke<void>(TOR_START_COMMAND);
}

export async function stopTor() {
  return invoke<void>(TOR_STOP_COMMAND);
}

export async function restartTor() {
  return invoke<void>(TOR_RESTART_COMMAND);
}

export async function requestNewIdentity() {
  return invoke<void>(TOR_NEW_IDENTITY_COMMAND);
}

export async function fetchRuntimeConfig() {
  return invoke<RuntimeConfigResponse>(GET_RUNTIME_CONFIG_COMMAND);
}

export async function saveRuntimeConfig(config: RuntimeConfigRequest) {
  return invoke<RuntimeConfigResponse>(SET_RUNTIME_CONFIG_COMMAND, {
    config,
  });
}
