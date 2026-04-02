import type {
  ControlAvailability,
  RuntimeStatus,
  TorRuntimeSnapshotDto,
} from './torq-api';

export type BadgeTone = 'success' | 'warning' | 'danger' | 'neutral' | 'muted';

export const statusToColor: Record<RuntimeStatus, BadgeTone> = {
  running: 'success',
  starting: 'warning',
  stopped: 'neutral',
  failed: 'danger',
};

export const controlAvailabilityToColor: Record<ControlAvailability, BadgeTone> = {
  unconfigured: 'neutral',
  unavailable: 'warning',
  available: 'success',
};

export function formatRuntimeStatus(status: RuntimeStatus) {
  const labels: Record<RuntimeStatus, string> = {
    stopped: 'Stopped',
    starting: 'Starting',
    running: 'Running',
    failed: 'Failed',
  };

  return labels[status];
}

export function formatBooleanStatus(value: boolean) {
  return value ? 'Available' : 'Unavailable';
}

export function booleanToColor(value: boolean): BadgeTone {
  return value ? 'success' : 'muted';
}

export function formatControlPortValue(port: ControlAvailability) {
  const labels: Record<ControlAvailability, string> = {
    unconfigured: 'Not configured',
    unavailable: 'Configured, unavailable',
    available: 'Available',
  };

  return labels[port];
}

export function formatBootstrapSource(snapshot: TorRuntimeSnapshotDto | null) {
  if (!snapshot) {
    return 'Unavailable';
  }

  if (snapshot.uses_control_bootstrap_observation) {
    return 'ControlPort';
  }

  if (snapshot.tor.status === 'starting' || snapshot.tor.status === 'running') {
    return 'Log-based';
  }

  return 'Unavailable';
}

export function bootstrapSourceToColor(snapshot: TorRuntimeSnapshotDto | null): BadgeTone {
  const source = formatBootstrapSource(snapshot);

  if (source === 'ControlPort') {
    return 'success';
  }

  if (source === 'Log-based') {
    return 'neutral';
  }

  return 'muted';
}
