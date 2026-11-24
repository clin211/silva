#!/usr/bin/env bash
# 创建一个随机邮箱、随机密码的后台账户，并打印凭据。
#
# 用法：server/scripts/create-user.sh [--role author|editor|admin]...
#   - --role 可重复指定以分配多个角色（如 --role author --role editor），默认 admin。
#   - 数据库地址取环境变量 DATABASE_URL，未设置时读 server/.env，再退回本地开发库。
#   - 本机有 cargo 时直接运行；否则使用 rust:1.100.0 Docker 镜像，
#     编译产物缓存在 Docker 卷 silva-cli-target 中，首次运行需要编译，后续很快。
#   - 运行前会先执行迁移（已执行的迁移会跳过）。
#   - 密码只经环境变量传给 CLI，不出现在命令行参数中；凭据只打印一次，请自行保存。
set -euo pipefail

usage() {
  echo "用法：$0 [--role author|editor|admin]..." >&2
  exit 2
}

roles=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --role)
      [[ $# -ge 2 ]] || usage
      case "$2" in
        author | editor | admin) roles+=("$2") ;;
        *) echo "未知角色：$2（可选 author、editor、admin）" >&2; exit 2 ;;
      esac
      shift 2
      ;;
    --role=*)
      set -- --role "${1#--role=}" "${@:2}"
      ;;
    -h | --help) usage ;;
    *) echo "未知参数：$1" >&2; usage ;;
  esac
done
[[ ${#roles[@]} -gt 0 ]] || roles=(admin)

SERVER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$SERVER_DIR"

if [[ -z "${DATABASE_URL:-}" && -f .env ]]; then
  DATABASE_URL="$(grep -E '^DATABASE_URL=' .env | tail -1 | cut -d= -f2- || true)"
fi
export DATABASE_URL="${DATABASE_URL:-postgres://silva:silva@127.0.0.1:55439/silva}"

# 随机凭据：邮箱前缀 8 位十六进制；密码 24 位字母数字（满足 12..1024 字节要求）。
suffix="$(openssl rand -hex 4)"
export SILVA_USER_EMAIL="user-${suffix}@silva.local"
export SILVA_USER_NAME="User ${suffix}"
export SILVA_USER_PASSWORD="$(openssl rand -base64 48 | tr -dc 'A-Za-z0-9' | head -c 24)"

role_args=()
for role in "${roles[@]}"; do role_args+=(--role "$role"); done

run_cli() {
  if command -v cargo >/dev/null 2>&1; then
    cargo run --quiet --locked -p silva-cli -- "$@"
  else
    docker run --rm --network host \
      -v "$SERVER_DIR":/work -w /work \
      -v silva-cargo-cache:/usr/local/cargo/registry \
      -v silva-cli-target:/tgt -e CARGO_TARGET_DIR=/tgt \
      -e CARGO_PROFILE_DEV_DEBUG=0 \
      -e DATABASE_URL -e SILVA_USER_EMAIL -e SILVA_USER_NAME -e SILVA_USER_PASSWORD \
      -e RUST_LOG=warn \
      rust:1.100.0 cargo run --quiet --locked -p silva-cli -- "$@"
  fi
}

echo "正在执行迁移并创建账户（首次运行需要编译，可能需要几分钟）..." >&2
run_cli migrate
run_cli create-user "${role_args[@]}"

cat <<EOF

账户已创建
  邮箱：$SILVA_USER_EMAIL
  密码：$SILVA_USER_PASSWORD
  姓名：$SILVA_USER_NAME
  角色：${roles[*]}
EOF
