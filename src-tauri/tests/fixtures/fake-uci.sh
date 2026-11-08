#!/bin/sh
# Minimal scripted UCI engine for tests. Plays e2e4 from the start position and
# e7e5 after 1.e4. "go infinite" searches until "stop"; "go nodes 13" crashes.
pos=""
searching=0
mv="e2e4"
while IFS= read -r line; do
  case "$line" in
    uci)
      echo "id name FakeFish 1"
      echo "id author OUTPOST tests"
      echo "option name Hash type spin default 16 min 1 max 1024"
      echo "option name Threads type spin default 1 min 1 max 8"
      echo "option name MultiPV type spin default 1 min 1 max 5"
      echo "option name Style type combo default normal var normal var wild"
      echo "uciok" ;;
    isready) echo "readyok" ;;
    position*)
      pos="$line"
      case "$pos" in *e2e4*) mv="e7e5" ;; *"b KQkq"*) mv="e7e5" ;; *) mv="e2e4" ;; esac ;;
    "go nodes 13") exit 3 ;;
    "go infinite")
      searching=1
      echo "info depth 1 seldepth 1 multipv 1 score cp 20 nodes 10 nps 100 time 1 pv $mv"
      echo "info depth 2 seldepth 2 multipv 1 score cp 25 lowerbound nodes 20 pv $mv" ;;
    go*)
      echo "info depth 1 seldepth 1 multipv 1 score cp 15 nodes 10 nps 100 time 1 pv $mv"
      echo "info depth 2 seldepth 2 multipv 1 score mate 3 nodes 30 nps 100 time 2 pv $mv"
      echo "bestmove $mv" ;;
    stop)
      if [ "$searching" = 1 ]; then searching=0; echo "bestmove $mv"; fi ;;
    quit) exit 0 ;;
  esac
done
