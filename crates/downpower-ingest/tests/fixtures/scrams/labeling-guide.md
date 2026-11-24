# Labeling guide: what started a reactor scram

Each record is an NRC Event Notification for a reactor scram (the header line gives the EN
number, the event date and each unit's SCRAM Code: auto_reactor and auto are automatic,
manual_reactor and manual are manual). Read the whole text, including any `* * * UPDATE` or
`EN Revision` paragraphs, and give the event two tags.

Label what the text says. Don't infer a cause the text doesn't state, and don't use outside
knowledge of the plant.

## System: the plant system the text says started the scram

Take the most upstream cause the text names, not the protective signal it produced:

- "a turbine trip caused by a loss of offsite power" is `offsite_power`;
- "low steam generator level after a main feed pump tripped" is `feedwater`;
- "a generator lockout from a main transformer fault" is `electrical`.

When the text names only the protective signal, the signal's own system:

- a turbine trip or a generator lockout: `turbine_generator`;
- low or high steam generator or reactor vessel water level: `feedwater`;
- low condenser vacuum: `cooling_water`;
- low reactor coolant flow, a reactor coolant or recirculation pump trip, high flux, high
  reactor pressure: `reactor_coolant`;
- undervoltage on a bus: `electrical`.

Values:

- `feedwater`: main feedwater and condensate: feed pumps, booster and condensate pumps, feed
  regulating valves and their controllers, feedwater heaters and heater drains, feedwater level
  control.
- `turbine_generator`: the main turbine and its controls (EHC or DEHC, stop, control, bypass and
  intercept valves, lube oil, trip fluid, moisture separator reheaters), the main generator,
  exciter, voltage regulator, generator protection and lockout relays, stator cooling, the
  isophase bus. A load rejection the grid didn't cause.
- `electrical`: the plant's own power distribution: main, unit auxiliary, station service and
  startup transformers, buses (vital AC and DC included), breakers, inverters, batteries and
  chargers, motor control centers, cables, and a fire in any of them.
- `offsite_power`: outside the plant's own distribution: the switchyard and its breakers,
  transmission lines, grid disturbances, a loss of offsite power caused off site.
- `cooling_water`: condenser vacuum and backpressure, circulating water and service water pumps,
  the intake and its screens, debris, marine life or ice at the intake, cooling towers.
- `reactor_coolant`: the reactor and its coolant system: control rods, their drives and rod
  control, reactor coolant pumps, recirculation pumps and flow control, the pressurizer and its
  pressure and level control, safety and relief valves, main steam isolation valves and main steam
  lines, reactor coolant leakage, and power or flux transients the text puts in the core.
- `instruments`: sensing and protection logic, when the text points at the instrument rather than
  a real condition elsewhere: a failed transmitter or sensor, a circuit card, a spurious or invalid
  protection signal, nuclear instrumentation, RPS or ESFAS logic faults.
- `not_named`: the text names no system that started it (the cause is under investigation, or it
  says only "automatic reactor trip"), or no plant system started it (a precautionary shutdown
  ahead of a hurricane, for one).

## Circumstance: what was going on that the text ties to the scram

- `weather`: lightning, storms, hurricanes, tornadoes, high winds, flooding, ice storms, extreme
  cold or heat, named as contributing.
- `testing`: a test or surveillance the text says caused or contributed. A test merely in progress,
  that the text doesn't connect to the trip, doesn't count.
- `maintenance`: maintenance, a modification, troubleshooting or other work the text says caused
  or contributed.
- `human_error`: an operator, technician or other personnel error, or a procedure error, the text
  names as a cause, outside testing and maintenance.
- `none`: none of these named.

When two apply, take the first in this order: weather, testing, maintenance, human_error.

## Output

One line per EN, in input order, tab-separated:

```
number	system	circumstance	sure	why
```

- `sure` is `yes`, or `no` when the text leaves you torn between two values.
- `why` is one short line (under 120 characters, no tabs) quoting or paraphrasing the decisive
  words, e.g. `trip of the A main feed pump; cause under investigation`.
