# What is it?
I want to build a CAM (Computer Aided Manufacturing) software for my hobby workshop. The name would be HobbyCAM. The application shall be able to produce CNC milling machine G codes based on 2D drawings.
The program shall be multi platform (at least Windows and Mac) and I want to make it open source (GPL license)

# My requirements
1. Shall be able to import 2D DXF drawings from Autocad and QCAD programs
2. 3D is out of scope.
3. The program shall be able to support 3 axis milling machines (milling and drilling). My milling machine doesn't automatic tool change option, so the tool change would be fully manual. No need to support other CNC machines (like lathe, plasma cutter, 3d printer)
4. Shall have a tool inventory where I can store and recall my tools (milling tools, drill bits).
5. Altough the main input is a 2D DXF file, it would be super useful to have basic drawing capabilities as well (circles, rectangles, text)
6. Automatic holding parts (those small pieces that prevents the pieces fly away during the milling) shall be implemented
7. No need to create automatic tool path selection. It's totaly OK, to select the elements (lines, areas) on the imported/created drawings and select the required operation (engrave, mill, drill) with the neccesary parameters (tool index, final depth, linear speed, tool rpm, depth increment)
8. A basic sanity check shall be implemented to catch the wrong tool path creation 
9. The program is definitely for hobby purposes only. Ignore every professional features.
10. I added a few images (images folder) from estlCAM program taht can be a good base model for my application. Please be sure not to clone the application as I want to avoid any legal issues
11. The application shall be self contained. Now extra libraries shall be installed on the user's machine. Rust language seems to be ideal for the project.
12. A basic simulation of tool moving shall be implemented as well
13. I need unit tests

# How to work?
1. We have to start with a planning session first. here ask all your questions that are not clear
2. Create an implemenation plan, and break down the project into smaller chunks
3. Before going to next steps, ensure all the tests are pass
4. Create user documentation as well
5. At the end I'd like to ask your help how to create a the release binaties on gitHUB (github actions) and how to submit it to Apple's App store
6. I want to do the project on agentic based. I need an architect agent, a coding and a testing agent. Please prepare the plan and workflow according
