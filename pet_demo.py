#!/usr/bin/env python3
"""ASCII Pet Demo — a simple interactive companion that responds to commands."""

import sys
import time

PETS = {
    "cat": r"""
  /\___/\
 (  o o  )
 (  =^=  )
  )     (
 (       )
( (  )  ( ) )
""",
    "dog": r"""
   ____
  /    \
 |  oo  |
 |  __  |
  \____/
   /  \
  /____\
""",
    "bunny": r"""
  (\(  ( -.-)
  o_(")(")
""",
}


def main():
    print("=== ASCII Pet Demo ===")
    print("Available pets:", ", ".join(PETS.keys()))
    print("Commands: pet name, switch <name>, quit\n")

    pet_name = "cat"
    while True:
        print(f"Your pet ({pet_name}):")
        print(PETS[pet_name])
        try:
            cmd = input("> ").strip().lower()
        except (EOFError, KeyboardInterrupt):
            print("\nGoodbye!")
            break

        if cmd in ("quit", "exit", "q"):
            print("Pet went to sleep. Bye!")
            break
        elif cmd.startswith("switch "):
            name = cmd.split(maxsplit=1)[1]
            if name in PETS:
                pet_name = name
                print(f"Switched to {name}!\n")
            else:
                print(f"Unknown pet. Available: {', '.join(PETS.keys())}\n")
        elif cmd.startswith("pet "):
            name = cmd.split(maxsplit=1)[1]
            if name in PETS:
                print(f"You pet the {name}. It purrs happily!\n")
            else:
                print(f"Unknown pet.\n")
        elif cmd == "dance":
            print("Your pet does a little dance!")
            for _ in range(3):
                print("(^_^) <  (>.<) > (^_^)")
                time.sleep(0.4)
            print()
        elif cmd == "feed":
            print(f"You feed the {pet_name}. It munches happily!\n")
        else:
            print("Commands: switch <name>, pet <name>, feed, dance, quit\n")


if __name__ == "__main__":
    main()
