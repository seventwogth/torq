import type {
  RuntimeConfigDto,
  RuntimeControlAuth,
  RuntimeLogMode,
} from './runtime-config';

export type RuntimeControlAuthKind = 'null' | 'cookie';

export interface RuntimeConfigFormState {
  torPath: string;
  logPath: string;
  logMode: RuntimeLogMode;
  torrcPath: string;
  useTorrc: boolean;
  workingDir: string;
  controlEnabled: boolean;
  controlHost: string;
  controlPort: string;
  controlAuth: RuntimeControlAuthKind;
  controlCookiePath: string;
}

export interface RuntimeConfigFormErrors {
  torPath?: string;
  logPath?: string;
  torrcPath?: string;
  workingDir?: string;
  controlHost?: string;
  controlPort?: string;
  controlCookiePath?: string;
}

export function createRuntimeConfigFormState(config: RuntimeConfigDto): RuntimeConfigFormState {
  const controlAuth = config.control?.auth;

  return {
    torPath: config.tor_path,
    logPath: config.log_path,
    logMode: config.log_mode,
    torrcPath: config.torrc_path ?? '',
    useTorrc: config.use_torrc,
    workingDir: config.working_dir ?? '',
    controlEnabled: config.control !== null,
    controlHost: config.control?.host ?? '',
    controlPort: config.control?.port?.toString() ?? '',
    controlAuth: normalizeControlAuth(controlAuth),
    controlCookiePath: controlAuth?.kind === 'cookie' ? controlAuth.cookie_path : '',
  };
}

export function validateRuntimeConfigForm(form: RuntimeConfigFormState): RuntimeConfigFormErrors {
  const errors: RuntimeConfigFormErrors = {};

  if (!isNonEmpty(form.torPath)) {
    errors.torPath = 'Tor executable path is required.';
  }

  if (!isNonEmpty(form.logPath)) {
    errors.logPath = 'Log path is required.';
  }

  if (form.useTorrc && !isNonEmpty(form.torrcPath)) {
    errors.torrcPath = 'torrc path is required when torrc usage is enabled.';
  }

  if (form.controlEnabled) {
    if (!isNonEmpty(form.controlHost)) {
      errors.controlHost = 'Control host is required.';
    }

    if (!isValidPort(form.controlPort)) {
      errors.controlPort = 'Control port must be a number between 1 and 65535.';
    }

    if (form.controlAuth === 'cookie' && !isNonEmpty(form.controlCookiePath)) {
      errors.controlCookiePath = 'Cookie path is required for cookie auth.';
    }
  }

  return errors;
}

export function hasRuntimeConfigFormErrors(errors: RuntimeConfigFormErrors): boolean {
  return Object.values(errors).some((value) => typeof value === 'string' && value.length > 0);
}

export function buildRuntimeConfigRequest(
  form: RuntimeConfigFormState,
  current: RuntimeConfigDto,
): RuntimeConfigDto {
  const control = form.controlEnabled
    ? {
        host: form.controlHost.trim(),
        port: Number.parseInt(form.controlPort, 10),
        auth:
          form.controlAuth === 'cookie'
            ? {
                kind: 'cookie' as const,
                cookie_path: form.controlCookiePath.trim(),
              }
            : ({ kind: 'null' as const } satisfies RuntimeControlAuth),
      }
    : null;

  return {
    tor_path: form.torPath.trim(),
    log_path: form.logPath.trim(),
    log_mode: form.logMode,
    torrc_path: normalizeOptionalString(form.torrcPath),
    use_torrc: form.useTorrc,
    args: [...current.args],
    working_dir: normalizeOptionalString(form.workingDir),
    control,
    stop_timeout_ms: current.stop_timeout_ms,
    log_poll_interval_ms: current.log_poll_interval_ms,
  };
}

function normalizeControlAuth(auth: RuntimeControlAuth | undefined): RuntimeControlAuthKind {
  return auth?.kind === 'cookie' ? 'cookie' : 'null';
}

function isNonEmpty(value: string): boolean {
  return value.trim().length > 0;
}

function isValidPort(value: string): boolean {
  if (!/^\d+$/.test(value.trim())) {
    return false;
  }

  const port = Number.parseInt(value, 10);
  return Number.isInteger(port) && port >= 1 && port <= 65535;
}

function normalizeOptionalString(value: string): string | null {
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}
