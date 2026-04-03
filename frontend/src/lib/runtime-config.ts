export type RuntimeLogMode = 'managed' | 'external';

export interface RuntimeControlAuthNull {
  kind: 'null';
}

export interface RuntimeControlAuthCookie {
  kind: 'cookie';
  cookie_path: string;
}

export type RuntimeControlAuth = RuntimeControlAuthNull | RuntimeControlAuthCookie;

export interface RuntimeControlConfig {
  host: string;
  port: number;
  auth: RuntimeControlAuth;
}

export interface RuntimeConfigDto {
  tor_path: string;
  log_path: string;
  log_mode: RuntimeLogMode;
  torrc_path: string | null;
  use_torrc: boolean;
  args: string[];
  working_dir: string | null;
  control: RuntimeControlConfig | null;
  stop_timeout_ms: number;
  log_poll_interval_ms: number;
}

export type RuntimeConfigRequest = RuntimeConfigDto;
export type RuntimeConfigResponse = RuntimeConfigDto;

export function isCookieAuth(
  auth: RuntimeControlAuth | null | undefined,
): auth is RuntimeControlAuthCookie {
  return auth?.kind === 'cookie';
}

export function isControlConfigured(config: RuntimeConfigDto | null | undefined) {
  return config?.control !== null;
}
