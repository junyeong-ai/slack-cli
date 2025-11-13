# Slack CLI

[![CI](https://github.com/junyeong-ai/slack-cli/workflows/CI/badge.svg)](https://github.com/junyeong-ai/slack-cli/actions)
[![Lint](https://github.com/junyeong-ai/slack-cli/workflows/Lint/badge.svg)](https://github.com/junyeong-ai/slack-cli/actions)
[![Rust](https://img.shields.io/badge/rust-1.91.1%2B%20(2024%20edition)-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Version](https://img.shields.io/badge/version-0.1.0-blue?style=flat-square)](https://github.com/junyeong-ai/slack-cli/releases)

> **🌐 한국어** | **[English](README.en.md)**

---

> **⚡ 빠르고 강력한 Slack 명령줄 도구**
>
> - 🚀 **밀리초 단위 검색** (SQLite FTS5 전문 검색)
> - 💾 **로컬 캐시** (사용자/채널 즉시 조회)
> - 🔍 **퍼지 매칭** (오타에도 정확한 검색)
> - 🛠️ **9개 명령어** (검색, 메시지, 설정 관리)

---

## ⚡ 빠른 시작 (1분)

```bash
# 1. 설치
git clone https://github.com/junyeong-ai/slack-cli
cd slack-cli
cargo build --release

# 2. 전역 설치 (선택사항)
./install.sh

# 3. 설정 초기화
slack config init --bot-token xoxb-your-token

# 4. 캐시 새로고침
slack cache refresh

# 5. 사용 시작! 🎉
slack users "john"
slack channels "general"
slack send "#general" "Hello team!"
```

**💡 Tip**: User token (`xoxp-`)을 사용하면 더 많은 기능을 사용할 수 있습니다.

---

## 🎯 주요 기능

### 🔍 강력한 검색
```bash
# 사용자 검색 (이름, 이메일, 표시명)
slack users "john" --limit 5

# 채널 검색 (이름, 주제, 설명)
slack channels "dev" --limit 10

# 메시지 검색 (워크스페이스 전체)
slack search "deadline" --channel "#dev-team"
```

### 💬 메시지 관리
```bash
# 채널에 메시지 전송
slack send "#general" "Meeting in 10 minutes"

# DM 전송
slack send "@john.doe" "Hello!"

# 스레드 답장
slack send "#dev-team" "Done!" --thread 1234567890.123456

# 채널 메시지 조회
slack messages "#general" --limit 20

# 스레드 전체 조회
slack thread "#dev-team" 1234567890.123456
```

### 📋 채널 관리
```bash
# 채널 멤버 목록
slack members "#dev-team"

# JSON 출력
slack channels "general" --json | jq
```

### ⚙️ 캐시 & 설정
```bash
# 캐시 상태 확인
slack cache stats

# 캐시 새로고침
slack cache refresh           # 전체
slack cache refresh users     # 사용자만
slack cache refresh channels  # 채널만

# 설정 관리
slack config show            # 설정 표시 (토큰 마스킹)
slack config path            # 설정 파일 경로
slack config edit            # 에디터로 수정
```

---

## 📦 설치

### Prerequisites
- Rust 1.91.1+ (2024 edition)
- Slack workspace 접근 권한

### 방법 1: 소스에서 빌드

```bash
git clone https://github.com/junyeong-ai/slack-cli
cd slack-cli
cargo build --release

# 바이너리 위치: target/release/slack
```

### 방법 2: 전역 설치

```bash
# 빌드 후 전역 설치
./install.sh

# 제거
./uninstall.sh
```

### 방법 3: Cargo

```bash
# 추후 지원 예정
cargo install slack-cli
```

---

## 🔑 Slack 토큰 생성

### User Token (권장) ⭐

1. [api.slack.com/apps](https://api.slack.com/apps) 접속
2. "Create New App" → "From scratch"
3. **User Token Scopes** 추가:
   ```
   channels:read channels:history groups:read groups:history
   im:read im:history mpim:read mpim:history
   users:read users:read.email chat:write search:read
   ```
4. "Install to Workspace" → 토큰 복사 (`xoxp-`로 시작)

### Bot Token (대안)

1. 위와 동일한 앱 생성
2. **Bot Token Scopes** 추가:
   ```
   channels:read channels:history groups:read groups:history
   im:read im:history mpim:read mpim:history
   users:read users:read.email chat:write
   ```
3. "Install to Workspace" → 토큰 복사 (`xoxb-`로 시작)

### 토큰 비교

| 기능 | User Token ⭐ | Bot Token |
|------|--------------|-----------|
| 채널 접근 | ✅ 자동 | ⚠️ 초대 필요 |
| 메시지 검색 | ✅ 가능 | ❌ 불가능 |
| 발신자 | 본인 | 봇 계정 |

---

## ⚙️ 설정

### 환경 변수

```bash
export SLACK_BOT_TOKEN="xoxb-..."      # 봇 토큰
export SLACK_USER_TOKEN="xoxp-..."    # 사용자 토큰 (권장)
```

### 설정 파일

**위치**:
- macOS: `~/.config/slack-cli/config.toml`
- Linux: `~/.config/slack-cli/config.toml`
- Windows: `%APPDATA%\slack-cli\config.toml`

**기본 설정** (`slack config init`로 생성):
```toml
bot_token = "xoxb-..."
user_token = "xoxp-..."

[cache]
ttl_users_hours = 24
ttl_channels_hours = 24
data_path = "~/.config/slack-cli/cache"  # 모든 플랫폼 동일

[retry]
max_attempts = 3
initial_delay_ms = 1000
max_delay_ms = 60000

[connection]
timeout_seconds = 30
max_idle_per_host = 10
```

### 설정 우선순위

```
CLI 플래그 > 환경 변수 > 설정 파일 > 기본값
```

**예시**:
```bash
# 설정 파일의 토큰 오버라이드
slack users "john" --token xoxp-temporary-token
```

---

## 🏗️ 아키텍처

### 핵심 기술

**빠른 검색**:
- **SQLite FTS5**: 전문 검색 엔진 (< 10ms 쿼리)
- **WAL 모드**: 읽기/쓰기 동시성
- **2단계 검색**: LIKE 정확 매칭 → FTS5 퍼지 매칭

**캐시 전략**:
- **전체 로드**: 서버 시작 시 모든 사용자/채널 캐싱
- **TTL 기반**: 24시간 후 자동 갱신
- **분산 락**: 다중 프로세스 안전성

**성능 최적화**:
- **Rust 2024**: 메모리 안전성 + 고성능
- **Tokio Async**: 비동기 I/O
- **Connection Pool**: HTTP 연결 재사용
- **Rate Limiting**: 자동 재시도 + 지수 백오프

### 시스템 구조

```
┌──────────────┐         ┌──────────────────┐         ┌─────────────┐
│   Terminal   │  stdin  │   Slack CLI      │  HTTPS  │    Slack    │
│   Commands   │◄───────►│   (clap/Tokio)   │◄───────►│  Workspace  │
│              │  stdout │                  │         │             │
└──────────────┘         └─────────┬────────┘         └─────────────┘
                                   │
                                   ▼
                          ┌─────────────────┐
                          │   SQLite Cache  │
                          │   (WAL + FTS5)  │
                          │                 │
                          │ • User FTS5     │
                          │ • Channel FTS5  │
                          │ • Distributed   │
                          │   locking       │
                          │ • Metadata      │
                          └─────────────────┘
```

### 왜 캐싱이 필요한가요?

**Slack API 제한**:
- 🚫 채널 이름 검색 API 없음
- ⏱️ Rate Limit 낮음 (Tier 2: 20 calls/min)
- 🐌 반복 쿼리 비효율적

**캐싱 솔루션**:
- 🚀 시작 시 전체 로드
- 🔍 로컬 FTS5 검색 (< 10ms)
- ⚡ API 호출 0회
- 🔄 TTL 기반 자동 갱신

**성능 비교**:

| 작업 | Slack API | 캐시 (FTS5) | 개선 |
|------|-----------|-------------|------|
| 사용자 검색 | ~500ms + rate limit | **<10ms** | **50배+** |
| 채널 검색 | ❌ 불가능 | **<10ms** | **가능** |
| 연속 쿼리 | Rate limit 제한 | **무제한** | **제한 없음** |

---

## 🔧 문제 해결

### 캐시가 갱신되지 않음

```bash
# 캐시 삭제 후 재생성
rm -rf ~/.local/share/slack-cli/cache  # Linux
rm -rf ~/Library/Application\ Support/slack-cli/cache  # macOS

# 다시 실행
slack cache refresh
```

### "Unauthorized" 오류

**확인 사항**:
- [ ] 토큰 형식 확인 (`xoxp-` 또는 `xoxb-`)
- [ ] 필수 scope 추가 확인
- [ ] Workspace 재설치 확인

**토큰 테스트**:
```bash
curl -H "Authorization: Bearer xoxp-YOUR-TOKEN" \
  https://slack.com/api/auth.test
```

### 메시지 검색 안 됨

**원인**: User token 없거나 `search:read` scope 없음

**해결**:
1. `SLACK_USER_TOKEN` 설정 (`xoxp-`)
2. `search:read` scope 추가
3. Workspace 재설치

### 디버그 로깅

```bash
RUST_LOG=debug slack users "john"
RUST_LOG=slack_cli::cache=trace slack cache refresh
```

### 캐시 데이터 확인

```bash
sqlite3 ~/.local/share/slack-cli/cache/slack.db

# 유용한 쿼리
SELECT COUNT(*) FROM users;
SELECT COUNT(*) FROM channels;
SELECT * FROM metadata;

# 캐시 신선도
SELECT
    key,
    datetime(CAST(value AS INTEGER), 'unixepoch') as last_sync,
    (unixepoch() - CAST(value AS INTEGER)) / 3600 as hours_ago
FROM metadata
WHERE key LIKE 'last_%_sync';
```

---

## 📚 명령어 참조

### slack users

사용자 검색 (이름, 이메일, 표시명)

```bash
slack users <query> [OPTIONS]

OPTIONS:
  --limit <N>      결과 개수 제한 [기본값: 10]
  --json           JSON 형식 출력
  --token <TOKEN>  임시 토큰 오버라이드

EXAMPLES:
  slack users "john"
  slack users "@gmail.com" --limit 20
  slack users "smith" --json | jq
```

### slack channels

채널 검색 (공개/비공개/DM/그룹 DM)

```bash
slack channels <query> [OPTIONS]

OPTIONS:
  --limit <N>  결과 개수 제한 [기본값: 10]
  --json       JSON 형식 출력

EXAMPLES:
  slack channels "dev"
  slack channels "general" --limit 5
```

### slack send

메시지 전송

```bash
slack send <channel> <text> [OPTIONS]

OPTIONS:
  --thread <TS>  스레드 타임스탬프 (답장)

EXAMPLES:
  slack send "#general" "Hello team!"
  slack send "@john.doe" "Hi John"
  slack send "#dev" "Fixed" --thread 1234567890.123456
```

### slack messages

채널 메시지 조회

```bash
slack messages <channel> [OPTIONS]

OPTIONS:
  --limit <N>  메시지 개수 [기본값: 100, 최대: 1000]
  --json       JSON 형식 출력

EXAMPLES:
  slack messages "#general"
  slack messages "#dev-team" --limit 50
```

### slack thread

스레드 전체 조회

```bash
slack thread <channel> <timestamp> [OPTIONS]

OPTIONS:
  --limit <N>  답장 개수 [기본값: 100]
  --json       JSON 형식 출력

EXAMPLES:
  slack thread "#general" 1234567890.123456
```

### slack members

채널 멤버 목록

```bash
slack members <channel> [OPTIONS]

OPTIONS:
  --json  JSON 형식 출력

EXAMPLES:
  slack members "#dev-team"
```

### slack search

메시지 검색 (워크스페이스 전체)

```bash
slack search <query> [OPTIONS]

OPTIONS:
  --channel <CH>  특정 채널로 제한
  --limit <N>     결과 개수 [기본값: 10]
  --json          JSON 형식 출력

EXAMPLES:
  slack search "deadline"
  slack search "bug" --channel "#dev-team"

NOTE: User token (xoxp-) + search:read scope 필요
```

### slack cache

캐시 관리

```bash
slack cache <COMMAND>

COMMANDS:
  stats    캐시 통계 (사용자/채널 개수)
  refresh  캐시 새로고침 [--users|--channels]
  path     캐시 파일 경로 출력

EXAMPLES:
  slack cache stats
  slack cache refresh
  slack cache refresh --users
  slack cache path
```

### slack config

설정 관리

```bash
slack config <COMMAND>

COMMANDS:
  init [OPTIONS]  설정 초기화
  show            설정 표시 (토큰 마스킹)
  path            설정 파일 경로
  edit            기본 에디터로 수정

EXAMPLES:
  slack config init --bot-token xoxb-...
  slack config show
  slack config edit
```

---

## 🚀 개발

### 빌드

```bash
git clone https://github.com/junyeong-ai/slack-cli
cd slack-cli

cargo build                # 개발 빌드
cargo build --release      # 최적화 빌드
cargo test                 # 테스트 실행 (65개)
cargo clippy              # 린트
cargo fmt                 # 포맷팅
```

### 프로젝트 구조

```
src/
├── main.rs              # 진입점: Tokio 런타임, 설정, CLI 실행
├── cli.rs               # clap 기반 CLI 명령어 정의
├── config.rs            # 설정 관리 (우선순위: CLI > ENV > File)
├── format.rs            # 출력 포맷팅 (텍스트/JSON)
├── cache/               # SQLite 캐시
│   ├── sqlite_cache.rs # 메인 구현
│   ├── schema.rs       # FTS5 스키마
│   ├── users.rs        # 사용자 캐싱
│   ├── channels.rs     # 채널 캐싱
│   ├── locks.rs        # 분산 락
│   └── helpers.rs      # 유틸리티
└── slack/              # Slack API 클라이언트
    ├── client.rs       # 통합 파사드
    ├── core.rs         # HTTP + Rate Limiting
    ├── users.rs        # 사용자 API
    ├── channels.rs     # 채널 API
    └── messages.rs     # 메시지 API
```

**개발자 가이드**: [CLAUDE.md](CLAUDE.md) - AI agent 특화 개발 문서

---

## 💬 지원

- **GitHub Issues**: [문제 신고](https://github.com/junyeong-ai/slack-cli/issues)
- **개발자 문서**: [CLAUDE.md](CLAUDE.md)

---

<div align="center">

**🌐 한국어** | **[English](README.en.md)**

**Version 0.1.0** • Rust 2024 Edition

Made with ❤️ for productivity

</div>
