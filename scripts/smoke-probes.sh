# shellcheck shell=bash
# scripts/smoke.sh 的一项一项探测，跟着插件走。没写的项会 FAIL 并给出写法，不会默认放行。
#
# 这个插件没有网页（清单没有 webui），网页和身份两项套件会记 N/A，所以只用写数据两项和附加检查。
# 请求从存储容器里发到它自己的 127.0.0.1：和核心转发进来走同一套接口和凭据，不用另开端口。

# 按核心给容器打的标签找本实例的存储容器。实例名没导出时按 SMOKE_BASE 的主机名第一段推。
_container() { # <插件 id> <服务名>
  local inst="${SMOKE_INSTANCE:-}" args=(--filter "label=wonton.plugin=$1" --filter "label=wonton.service=$2")
  if [[ -z "$inst" && -n "${SMOKE_BASE:-}" ]]; then
    inst="${SMOKE_BASE#http://}"
    inst="${inst%%.*}"
  fi
  [[ -z "$inst" ]] || args+=(--filter "label=wonton.instance=${inst//-/}")
  docker ps --format '{{.Names}}' "${args[@]}" | head -1
}

_store() {
  local name
  name="$(_container app.wonton.personal-data store)"
  [[ -n "$name" ]] || {
    echo "找不到存储容器（插件 id app.wonton.personal-data、服务 store）" >&2
    return 1
  }
  printf '%s' "$name"
}

# 凭据命令就是核心安装时用的那条：从标准输入拿值，写进数据库。探针自己签一把。
_key() { # <存储容器>
  printf 'probe-key' | docker exec "$1" /app/personal-data credentials issue probe
}

write_point() { # <存储容器> <凭据> <tid>：照契约写一个点
  docker exec "$1" wget -q -O - \
    --header "Authorization: Bearer $2" \
    --header 'Content-Type: application/json' \
    --header 'X-Wonton-User: @probe:example.org' \
    --post-data "{\"_type\":\"location\",\"lat\":1.5,\"lon\":2.5,\"acc\":5,\"tst\":$(date +%s),\"tid\":\"$3\"}" \
    http://127.0.0.1:8090/location
}

data_write() {
  local store marker key
  store="$(_store)" || return 1
  marker="probe-$RANDOM"
  key="$(_key "$store")" || return 1
  printf '%s' "$marker" >"$SMOKE_STATE"
  write_point "$store" "$key" "$marker" | grep -q '"accepted":1' || {
    echo "写点没被收下" >&2
    return 1
  }
}

data_check() {
  local store marker key body
  store="$(_store)" || return 1
  marker="$(cat "$SMOKE_STATE")"
  key="$(_key "$store")" || return 1
  body="$(docker exec "$store" wget -q -O - \
    --header "Authorization: Bearer $key" \
    --header 'X-Wonton-User: @probe:example.org' \
    "http://127.0.0.1:8090/query?type=location&from=0&to=$(date +%s%3N)")" || return 1
  grep -qF "$marker" <<<"$body" || {
    echo "重装后读不回写进去的点：$(head -c 300 <<<"$body")"
    return 1
  }
}

extra_check() { # 凭据和请求头不对时分开报，别把两种原因混成一个
  local store code
  store="$(_store)" || return 1
  code="$(docker exec "$store" wget -q -S -O /dev/null 'http://127.0.0.1:8090/query?type=location&from=0&to=1' 2>&1 |
    grep -o 'HTTP/1.1 [0-9]*' | tail -1 | awk '{print $2}')"
  [[ "$code" == 403 ]] || {
    echo "不带凭据应当是 403，实际是 ${code:-没回应}"
    return 1
  }
  code="$(docker exec "$store" wget -q -S -O /dev/null \
    --header "Authorization: Bearer $(_key "$store")" \
    'http://127.0.0.1:8090/query?type=location&from=0&to=1' 2>&1 |
    grep -o 'HTTP/1.1 [0-9]*' | tail -1 | awk '{print $2}')"
  [[ "$code" == 400 ]] || {
    echo "有凭据但缺 X-Wonton-User 应当是 400，实际是 ${code:-没回应}"
    return 1
  }
  extra_item PASS 接口 "凭据和请求头分开校验"
}
