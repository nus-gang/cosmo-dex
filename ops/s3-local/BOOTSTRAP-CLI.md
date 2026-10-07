# 초기화 전 검사 CLI

현재 run의 Paperclip 인증 환경에서 실행한다. 토큰을 인자나 파일로 전달하지 않는다.

```sh
python3 -B ops/s3-local/bootstrap_cli.py \
  --bundle /absolute/bundle --artifacts /absolute/artifacts \
  --input-set /absolute/inputs/input.json \
  --effective-profile /absolute/inputs/effective-profile.json \
  --scratch /absolute/private-scratch \
  --runtime-pin APPROVED_MANIFEST_SHA256 \
  --local-demo-profile s3-dev-local/1 --acknowledge-unproven-space \
  --native-decision-id FINAL_DECISION_UUID \
  --ceo-revision CEO_REVISION_UUID --cto-revision CTO_REVISION_UUID
```

표시한 SHA/UUID는 실제 독립 승인 참조로 치환해야 한다. 현재 runtime pin은 미발급이다.
명령은 인증 audit → capture/descriptor SHA → private checker/C 의미 검증 → 새 audit를 호출한다.
성공 보고서는 시점 한정 검사 결과이며 재사용 기동 허가가 아니다. home/key 생성·RPC·서비스 기동은 없다.
거절은 exit 2, stdout 비움, stderr `LOCAL_BOOTSTRAP_CHECK_REJECTED`다.
home 생성 CLI와 실제 chain launcher 연결은 아직 남아 있다.
