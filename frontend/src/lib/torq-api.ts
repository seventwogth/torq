import { invoke } from '@tauri-apps/api/core';

export const TOR_STATE_COMMAND = 'tor_state';
export const TOR_RUNTIME_SNAPSHOT_COMMAND = 'tor_runtime_snapshot';
export const TOR_START_COMMAND = 'tor_start';
export const TOR_STOP_COMMAND = 'tor_stop';
export const TOR_RESTART_COMMAND = 'tor_restart';
export const TOR_NEW_IDENTITY_COMMAND = 'tor_new_identity';

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
