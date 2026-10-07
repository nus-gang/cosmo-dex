# F04 Attempt crash 경계

`runtime/attempt_crash.rs`는 fault build의 격리 child에서만 호출하는 F04 전용
driver다. 일반 scheduler·worker·REST에는 자동 연결하지 않는다.

경계는 C store의 `after_wal_sync` 하나로 고정한다. 이 시점에는 TxRaw 객체와
Attempt WAL frame의 fsync가 끝났지만 `commit.dev.json` marker 쓰기는 시작하지
않았다. child는 exact Attempt/TxRaw·Observation·시각과
`UNKNOWN_TAIL_NO_NEW_ENVELOPE` 기대값을 먼저 private evidence root에 no-replace
예약하고 fsync한 뒤 hook을 설치한다. hook 도달 시 `_exit(86)`이며 Drop·복구·재시도·
서명·방송·새 봉투 생성은 없다.

component 시험은 fee0/25에서 실제 C/L-D `Command::Attempt`를 실행해 exit86,
marker 불변, WAL tail 증가, `transaction.dev` 보존, 두 번 reopen의 동일
`UNKNOWN_OR_INCOMPLETE_STORE`, 보고서/원 bytes 불변을 확인한다. 이 C 오류는
잔존 transaction을 먼저 발견한 fail-closed 구현 코드이며 계약 결과
`UNKNOWN_TAIL_NO_NEW_ENVELOPE`를 축소하지 않는다. 이 결과는 경계 준비 근거이며
전원 손실 모의, 실제 chain/RPC/방송, 3회 반복, DEV09/F04 PASS, runtime 승인이나
복구 허가가 아니다. UNKNOWN tail은 자동 truncate/repair하지 않는다.
