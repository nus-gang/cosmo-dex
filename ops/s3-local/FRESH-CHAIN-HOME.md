# 새 Chain home 게시 경계

`fresh_chain_home.publish`는 호출자가 검증한 guard/genesis/새 private key/초기 서명 상태 원문을 새 home에 저장하는 내부 함수다. 공개 fixture 키를 실제 서비스에 사용하지 않는다. genesis 생성·경제 의미 검증·키 생성·독립 승인·최종 CLI 연결은 아직 남아 있다.

- 현재 UID의 canonical private parent0700 아래 새 root0700만 생성한다. 기존 파일/디렉터리/링크는 교체하지 않는다.
- guard 파일0600의 exclusive 생성→file fsync→root fsync가 끝난 뒤 config/data0700과 각 파일0600을 게시한다. 디렉터리와 parent도 fsync한다.
- 오류/중단 뒤 자동 삭제·재시도·수리는 없다. 부분 home을 보존하며 같은 경로 재호출은 거절된다. 운영자는 원시 증거를 보존한 뒤 별도 새 경로를 준비해야 한다.
- private parent는 신뢰하는 단일 사용자 관리 경계다. 같은 UID의 적대적 동시 수정에 대한 sandbox가 아니다. service DB/writer lock/listener를 생성하지 않는다.

검증: `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s ops/s3-local -p test_fresh_chain_home.py -v` — 4 PASS/0 FAIL. 합성 bytes·임시 filesystem 시험이다. 승인 B/C 의미 검증, 실제 genesis/home 초기화 전체, 서비스 기동, runtime 승인 또는 DEV PASS가 아니다.

다음 SRE 연결점은 새 키/공개 등록 자료와 B의 genesis 검증, C guard/store 생성 API의 준비 경로다. chain 관리 launcher/fault/정리·최종 build/manifest·CEO/CTO 독립 승인·CTO→Security 심사는 미완료다.
