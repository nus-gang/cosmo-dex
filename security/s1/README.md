# S1 독립 보안 시험

NUS-23 Security가 작성한 시험 코드. 제품 코드 변경 없음. CTO 독립 리뷰가 필요하다.

저장소 루트에서 `chain/app/scripts/build.sh`의 Go 1.26.5 고정 빌드를 먼저 수행하고 `web`에서 `npm ci --ignore-scripts`로 lock을 설치한다. 그 후:

```sh
python3 security/s1/live.py --binary "$PWD/chain/app/bin/nusd" --home "$PAPERCLIP_RUN_SCRATCH_DIR/security-devnet" --output "$PWD/.evidence/security"
```

Paperclip 밖에서는 `--home`에 새로운 시험 전용 디렉터리를 지정한다. 기존 home을 재사용하거나 초기화하지 않는다. 포트 32656~32687을 사용하는 bounded 시험이며 finally에서 네 검증인을 종료한다. CI의 home은 runner.temp이다. 별도 설치 의존성 경로는 `NUS_TEST_DEPENDENCIES`로 지정할 수 있다.

ML-DSA는 lock에 고정된 noble 0.4.1 구현을 쓴다. 공개 fixture seed로만 TX를 생성하며 개인키 입력/출력 기능이 없다. SignDoc/TxRaw의 protobuf framing은 부정 메시지를 구성하기 위한 시험 코드이며 새 암호 알고리즘이 아니다. 실제 Cosmos SDK 검증에 통과한 긍정 대조군을 먼저 실행한다. 테스트 계정은 정렬 순서가 아닌 account_number로 찾는다.

39건: 정상 입출금 대조군, chain/account/signature/owner/등록키, payer/granter/fee/gas, bank 우회, genesis, 정수 경계, 만료, 초과출금, 두 계정 입출금, 동일 TxRaw, stale epoch, 같은 sequence의 두 사전 서명 출금, 직접 서명 회수, 최대 금액 및 잔고 소진. 거절은 원인 문자열도 대조한다. 모든 건에서 DEVQUOTE/DEVGAS 보존을 검증하며 확정된 실패는 가스 1000 및 sequence 1 소비, quote/epoch 불변을 확인한다.

`race_first/race_second`는 같은 상태에서 두 TX를 미리 서명한 후 순서대로 제출한다. 같은 블록의 병렬 실행이나 네트워크 분할 시험은 아니다. 최대 epoch까지 실제 TX를 반복하지 않으며 도달 불가능 경계는 기존 keeper 단위시험/코드 검토 범위다. 직접 회수는 정상 quorum의 직접 SDK 서명 출금이며 독립 비상 회수나 분산 WAL 증명이 아니다.

`result.json`, 공개 genesis/config, 공개 TxRaw와 supervisor 로그만 업로드한다. home, validator 키, journal/DB는 제외한다. 테스트 PASS와 main 인수/출시 승인을 구별한다.
