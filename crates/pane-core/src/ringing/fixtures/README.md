# Sound fixtures

The ringing tests' compressed sounds (#559): half a second of an 880 Hz
sine tone at 22,050 Hz, mono, made for Pane with ffmpeg's own tone
source, so they are Pane's, under its licence, and record nothing:

```sh
ffmpeg -f lavfi -i "sine=frequency=880:sample_rate=22050:duration=0.5" -ac 1 \
  -c:a libmp3lame -b:a 32k -map_metadata -1 -write_xing 0 -id3v2_version 0 tone.mp3
ffmpeg -f lavfi -i "sine=frequency=880:sample_rate=22050:duration=0.5" -ac 1 \
  -c:a libvorbis -q:a 0 -map_metadata -1 -fflags +bitexact -flags:a +bitexact tone.ogg
```

They are compiled into the tests only, never into Pane.
