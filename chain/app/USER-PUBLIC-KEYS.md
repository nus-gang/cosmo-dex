# 사용자 공개키 genesis · SRE/Web 인계

NUS-27: 로컬 합성 자산 전용. protocol/v1 계약, S0, 계정 초기 할당량은 변경하지 않는다.

`--user-public-keys users.json`은 **파일 경로**이며 파일 내용은 다음 JSON 배열이다.

```json
["<user0 canonical base64 public key>", "<user1 canonical base64 public key>"]
```

정확히 두 문자열, 각 키는 ML-DSA-65 1952 bytes를 표준 base64(RFC 4648 alphabet, padding 포함)로 인코딩한다. 줄바꿈·공백이 키 문자열 내부에 있거나 padding bits가 비정규이면 거부한다. JSON 바깥쪽 공백은 허용한다. 객체·null·숫자·다른 개수·추가 JSON 값·잘못된 길이·중복 키를 거부한다. genesis 검증은 사용자/운영자/모듈/disabled-authority 주소 충돌을 거부한다. 검증 실패 시 node home을 생성하지 않는다. 키·입력 내용을 오류에 되풀이하지 않는다.

```sh
cd chain/app
sh scripts/build.sh
bin/nusd init --home .nus-user-demo \
  --operator-accounts config/operator-accounts.json \
  --user-public-keys users.json > init.json
# init.json의 genesis_hash를 사용
bin/nusd start --home .nus-user-demo --genesis-hash '<genesis_hash>'
```

출력 `users`는 실제 제공한 공개키의 주소이며 배열 순서를 유지한다. 계정 번호·sequence·epoch는 snapshot에서 주소로 조회한다. 사용자의 개인키/seed는 지갑 안에서 생성·보관하고 이 CLI나 파일에 전달하지 않는다. 외부 지갑이 DIRECT로 서명한 TxRaw는 기존 `nusd broadcast --file signed.tx`로 제출할 수 있다. `nusd tx --user 0|1` 및 `receipt --user`는 계속 공개 fixture 전용이므로 사용자 키 genesis에서는 지갑의 서명/주소 조회를 사용한다.

플래그 생략 시 기존 공개 fixture 키 두 개와 기존 smoke 동작을 유지한다. 키 등록 TX·키 복구·실자산 기능을 추가하지 않는다. 앱 버전과 wire 계약도 그대로다.

## SRE reset 절차

1. 연결된 지갑·제출 프로세스와 개발망 validator를 모두 정지한다.
2. 기존 home은 덮어쓰지 않는다. 보관이 필요하면 별도 이름으로 옮기고 새 빈 home 경로를 선택한다. 다중 검증인은 SRE의 기존 reset 절차로 모든 노드의 data/validator state를 함께 초기화한다. 운영망에서 실행하지 않는다.
3. 지갑이 생성한 공개키 두 개만 JSON으로 내보내고 위 init을 한 번 실행한다. 운영자 입력은 별도 파일로 유지한다.
4. 다중 검증인 genesis 조립 시 `app_state.public_keys`와 운영자 할당을 모든 노드에 동일하게 반영한다. 최종 genesis 바이트 확정 후 hash를 다시 계산하여 모든 start와 지갑 연결 설정에 pin한다.
5. 이전 genesis의 서명 TX·sequence·epoch·receipt 캐시를 폐기한다. 새 snapshot의 genesis hash와 주소를 확인하고 새로운 request_id로 입금/출금을 검증한다. 기존 genesis/data를 유지한 채 키만 바꾸면 안 된다.

## 재현 검증

Go 1.26.5를 PATH에 둔다.

```sh
sh scripts/build.sh
NUSD_BINARY="$PWD/bin/nusd" go test -mod=readonly -v ./...
python3 scripts/user-public-keys-test.py --binary bin/nusd \
  --operator-accounts config/operator-accounts.json --output /path/to/new-validation-dir
python3 scripts/genesis-input-test.py --binary bin/nusd \
  --operator-accounts config/operator-accounts.json --output /path/to/new-operator-dir
python3 scripts/smoke.py --binary bin/nusd \
  --operator-accounts config/operator-accounts.json --output /path/to/new-smoke-dir
```

`TestUserPublicKeysCLIDirect`는 메모리에서 두 무작위 키를 생성하고 공개 부분만 CLI genesis로 전달한다. 그 genesis로 실제 SDK BaseApp을 초기화하고 두 사용자의 DIRECT 입금/출금 총 4건을 FinalizeBlock/Commit한다. 각 커밋에서 자산 보존을 검사한다. 개인키를 파일·로그로 내보내지 않는다. 이는 실제 SDK 서명/상태 전이 검증이며 다중 검증인 합의·브라우저 시연의 통과를 뜻하지 않는다.
